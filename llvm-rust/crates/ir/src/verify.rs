//! Structural verifier for the first Track B IR slice.

use llvm_rust_support::Error;

use crate::constant::ConstKind;
use crate::function::{Function, LocalId};
use crate::inst::{InstKind, Instruction};
use crate::linkage::Linkage;
use crate::module::Module;
use crate::ty::Type;
use crate::value::{Operand, OperandKind};

/// Verification error type.
pub type VerifyError = Error;

/// Verifies module structure. Returns every error found.
pub fn verify_module(module: &Module) -> Result<(), Vec<VerifyError>> {
    let mut errors = Vec::new();

    for (id, ty) in module.types.iter() {
        if let Type::Struct {
            name: Some(name), ..
        } = ty
        {
            if !module.types.is_defined(id) {
                errors.push(Error::verify(format!(
                    "type %{name} is referenced but never defined"
                )));
            }
        }
    }

    for (_, global) in module.globals.iter() {
        if !module.types.is_valid(global.ty) {
            if module.function_named(global.name).is_some() {
                continue;
            }
            errors.push(Error::verify(format!(
                "global @{} is referenced but never defined",
                module.name(global.name)
            )));
            continue;
        }
        match global.init {
            Some(init) => {
                let constant = module.constant(init);
                if constant.ty != global.ty {
                    errors.push(Error::verify(format!(
                        "initializer type for @{} does not match the global type",
                        module.name(global.name)
                    )));
                }
            }
            None => {
                if global.linkage != Linkage::External {
                    errors.push(Error::verify(format!(
                        "global @{} has non-external linkage but no initializer",
                        module.name(global.name)
                    )));
                }
            }
        }
    }

    for (id, function) in module.functions.iter() {
        if !module.types.is_valid(function.ty) {
            if module.global_named(function.name).is_some() {
                continue;
            }
            errors.push(Error::verify(format!(
                "function @{} is referenced but never defined",
                module.name(function.name)
            )));
            continue;
        }
        let Some((ret, params, _)) = module.function_type(id) else {
            errors.push(Error::verify(format!(
                "function @{} does not have a function type",
                module.name(function.name)
            )));
            continue;
        };
        if params.len() != function.params.len() {
            errors.push(Error::verify(format!(
                "function @{} parameter count does not match its type",
                module.name(function.name)
            )));
        } else {
            for (index, param) in function.params.iter().enumerate() {
                if param.ty != params[index] {
                    errors.push(Error::verify(format!(
                        "parameter {index} of @{} does not match the function type",
                        module.name(function.name)
                    )));
                }
            }
        }
        if function.is_declaration {
            continue;
        }
        if function.entry.is_none() {
            errors.push(Error::verify(format!(
                "function @{} has a body but no entry block",
                module.name(function.name)
            )));
        }
        verify_function(module, function, ret, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn verify_function(
    module: &Module,
    function: &Function,
    ret: crate::ty::TypeId,
    errors: &mut Vec<VerifyError>,
) {
    for (_, block) in function.blocks.iter() {
        if block.insts.is_empty() {
            let label = block.name.map(|name| module.name(name)).unwrap_or("0");
            errors.push(Error::verify(format!(
                "block %{label} in @{} has no terminator",
                module.name(function.name)
            )));
            continue;
        }
        for (index, &inst_id) in block.insts.iter().enumerate() {
            let inst = &function.insts[inst_id];
            let is_last = index + 1 == block.insts.len();
            if !is_last && inst.kind.is_terminator() {
                errors.push(Error::verify(format!(
                    "instruction after terminator in @{}",
                    module.name(function.name)
                )));
            }
            if is_last && !inst.kind.is_terminator() {
                errors.push(Error::verify(format!(
                    "block in @{} does not end with a terminator",
                    module.name(function.name)
                )));
            }
            if let Some(result) = inst.result {
                if result as usize >= function.locals.len() {
                    errors.push(Error::verify(format!(
                        "instruction result in @{} is out of range",
                        module.name(function.name)
                    )));
                } else if function.local(result).def != Some(inst_id) {
                    errors.push(Error::verify(format!(
                        "instruction result in @{} does not point back to the instruction",
                        module.name(function.name)
                    )));
                }
            }
            verify_instruction(module, function, inst, ret, errors);
        }
    }
}

fn verify_instruction(
    module: &Module,
    function: &Function,
    inst: &Instruction,
    ret: crate::ty::TypeId,
    errors: &mut Vec<VerifyError>,
) {
    match &inst.kind {
        InstKind::Binary { lhs, rhs, .. } | InstKind::ICmp { lhs, rhs, .. } => {
            check_operand(module, function, *lhs, errors, "operand");
            check_operand(module, function, *rhs, errors, "operand");
        }
        InstKind::FCmp { lhs, rhs, .. } => {
            check_operand(module, function, *lhs, errors, "operand");
            check_operand(module, function, *rhs, errors, "operand");
        }
        InstKind::FNeg { value, .. } => {
            check_operand(module, function, *value, errors, "operand");
        }
        InstKind::Alloca { elem_ty, count, .. } => {
            if *elem_ty as usize >= module.types.len() {
                errors.push(Error::verify("alloca element type is out of range"));
            }
            if let Some(count) = count {
                check_operand(module, function, *count, errors, "alloca count");
            }
        }
        InstKind::Load { ptr, .. } => {
            check_operand(module, function, *ptr, errors, "load pointer");
        }
        InstKind::Store { value, ptr, .. } => {
            check_operand(module, function, *value, errors, "store value");
            check_operand(module, function, *ptr, errors, "store pointer");
        }
        InstKind::Gep {
            source_ty,
            ptr,
            indices,
            ..
        } => {
            if *source_ty as usize >= module.types.len() {
                errors.push(Error::verify("getelementptr source type is out of range"));
            }
            check_operand(module, function, *ptr, errors, "getelementptr pointer");
            for index in indices {
                check_operand(module, function, index.value, errors, "getelementptr index");
            }
        }
        InstKind::Call { callee, args, .. } => {
            check_operand(module, function, *callee, errors, "call callee");
            for arg in args {
                check_operand(module, function, *arg, errors, "call argument");
            }
            if let OperandKind::Function(id) = callee.kind {
                if let Some(function_ref) = module.functions.try_get(id) {
                    if module.types.is_valid(function_ref.ty) {
                        if let Some((_, params, varargs)) = module.function_type(id) {
                            if !varargs && params.len() != args.len() {
                                errors.push(Error::verify(format!(
                                    "call to @{} has the wrong number of arguments",
                                    module.name(function_ref.name)
                                )));
                            }
                        }
                    }
                }
            }
        }
        InstKind::Phi { incoming } => {
            let expected = inst.result.map(|local| function.local_type(local));
            for (value, block) in incoming {
                check_operand(module, function, *value, errors, "phi value");
                check_block(function, *block, errors, "phi predecessor");
                if let Some(expected) = expected {
                    if value.ty != expected {
                        errors.push(Error::verify(
                            "phi value type does not match the result type",
                        ));
                    }
                }
            }
        }
        InstKind::Select {
            cond,
            on_true,
            on_false,
            ..
        } => {
            check_operand(module, function, *cond, errors, "select condition");
            check_operand(module, function, *on_true, errors, "select value");
            check_operand(module, function, *on_false, errors, "select value");
        }
        InstKind::Cast { value, to, .. } => {
            check_operand(module, function, *value, errors, "cast value");
            if *to as usize >= module.types.len() {
                errors.push(Error::verify("cast target type is out of range"));
            }
        }
        InstKind::Ret { value } => match value {
            Some(value) => {
                check_operand(module, function, *value, errors, "return value");
                if value.ty != ret {
                    errors.push(Error::verify(format!(
                        "return type does not match in @{}",
                        module.name(function.name)
                    )));
                }
            }
            None => {
                if !module.types.get(ret).is_void() {
                    errors.push(Error::verify(format!(
                        "missing return value in @{}",
                        module.name(function.name)
                    )));
                }
            }
        },
        InstKind::Br { dest } => check_block(function, *dest, errors, "branch target"),
        InstKind::CondBr {
            cond,
            then_block,
            else_block,
        } => {
            check_operand(module, function, *cond, errors, "branch condition");
            if !matches!(module.types.get(cond.ty), Type::Int(1)) {
                errors.push(Error::verify("conditional branch condition is not i1"));
            }
            check_block(function, *then_block, errors, "branch target");
            check_block(function, *else_block, errors, "branch target");
        }
        InstKind::Switch {
            value,
            default,
            cases,
        } => {
            check_operand(module, function, *value, errors, "switch value");
            check_block(function, *default, errors, "switch default");
            for (case, block) in cases {
                let constant = module.constant(*case);
                if constant.ty != value.ty {
                    errors.push(Error::verify(
                        "switch case type does not match the value type",
                    ));
                }
                check_block(function, *block, errors, "switch case target");
            }
        }
        InstKind::Unreachable => {}
    }
}

fn check_operand(
    module: &Module,
    function: &Function,
    operand: Operand,
    errors: &mut Vec<VerifyError>,
    what: &str,
) {
    match operand.kind {
        OperandKind::Local(id) => {
            if id as usize >= function.locals.len() {
                errors.push(Error::verify(format!("{what} local value is out of range")));
            } else if function.local_type(id) != operand.ty {
                errors.push(Error::verify(format!(
                    "{what} type does not match the local value"
                )));
            } else {
                let local = function.local(id);
                if !local.is_param && local.def.is_none() {
                    errors.push(Error::verify(format!(
                        "{what} refers to a local value that is never defined"
                    )));
                }
            }
        }
        OperandKind::Const(id) => match module.constants.try_get(id) {
            Some(constant) if constant.ty == operand.ty => {}
            Some(_) => errors.push(Error::verify(format!(
                "{what} type does not match the constant"
            ))),
            None => errors.push(Error::verify(format!("{what} constant is out of range"))),
        },
        OperandKind::Global(id) => match module.globals.try_get(id) {
            Some(global) if module.types.is_valid(global.ty) => {}
            Some(_) => errors.push(Error::verify(format!(
                "{what} refers to an undefined global"
            ))),
            None => errors.push(Error::verify(format!("{what} global is out of range"))),
        },
        OperandKind::Function(id) => match module.functions.try_get(id) {
            Some(function_ref) if module.types.is_valid(function_ref.ty) => {}
            Some(_) => errors.push(Error::verify(format!(
                "{what} refers to an undefined function"
            ))),
            None => errors.push(Error::verify(format!("{what} function is out of range"))),
        },
    }
}

fn check_block(
    function: &Function,
    block: crate::function::BlockId,
    errors: &mut Vec<VerifyError>,
    what: &str,
) {
    if block.index() >= function.blocks.len() {
        errors.push(Error::verify(format!("{what} is out of range")));
    }
}

#[allow(dead_code)]
fn constant_is_valid(module: &Module, id: crate::constant::ConstId) -> bool {
    match module.constants.try_get(id) {
        Some(constant) => match constant.kind {
            ConstKind::Global(global) => module.globals.try_get(global).is_some(),
            ConstKind::Function(function) => module.functions.try_get(function).is_some(),
            _ => true,
        },
        None => false,
    }
}

#[allow(dead_code)]
fn local_is_defined(function: &Function, id: LocalId) -> bool {
    (id as usize) < function.locals.len()
}
