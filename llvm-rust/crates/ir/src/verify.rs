//! Structural verifier for the first Track B IR slice.

use llvm_rust_support::Error;

use crate::constant::{ConstExpr, ConstKind};
use crate::function::{Function, LocalId};
use crate::inst::{InstKind, Instruction, LandingPadClause};
use crate::linkage::Linkage;
use crate::module::{Global, GlobalKind, Module};
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

    for (_, constant) in module.constants.iter() {
        match &constant.kind {
            ConstKind::Expr(expr) => verify_const_expr(module, constant.ty, expr, &mut errors),
            ConstKind::BlockAddress { function, block } => {
                verify_block_address(module, constant.ty, *function, *block, &mut errors);
            }
            ConstKind::InlineAsm { fn_ty, .. } => {
                if !matches!(module.types.get(constant.ty), Type::Pointer) {
                    errors.push(Error::verify("inline asm type is not a pointer"));
                }
                if !module.types.is_valid(*fn_ty)
                    || !matches!(module.types.get(*fn_ty), Type::Function { .. })
                {
                    errors.push(Error::verify("inline asm type is not a function type"));
                }
            }
            _ => {}
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
        if global.kind.is_alias() {
            let valid_linkage = if global.kind == GlobalKind::IFunc {
                matches!(
                    global.linkage,
                    Linkage::External
                        | Linkage::Private
                        | Linkage::Internal
                        | Linkage::LinkOnce
                        | Linkage::Weak
                        | Linkage::LinkOnceOdr
                        | Linkage::WeakOdr
                )
            } else {
                matches!(
                    global.linkage,
                    Linkage::External
                        | Linkage::Private
                        | Linkage::Internal
                        | Linkage::LinkOnce
                        | Linkage::Weak
                        | Linkage::LinkOnceOdr
                        | Linkage::WeakOdr
                        | Linkage::AvailableExternally
                )
            };
            if !valid_linkage {
                errors.push(Error::verify(format!(
                    "{} @{} has invalid linkage",
                    if global.kind == GlobalKind::IFunc {
                        "ifunc"
                    } else {
                        "alias"
                    },
                    module.name(global.name)
                )));
            }
            match global.init {
                Some(init) => {
                    if !matches!(module.types.get(module.constant(init).ty), Type::Pointer) {
                        errors.push(Error::verify(format!(
                            "alias @{} target is not a pointer",
                            module.name(global.name)
                        )));
                    } else if global.kind == GlobalKind::IFunc {
                        verify_ifunc_resolver(module, global, init, &mut errors);
                    } else {
                        verify_alias(module, global, init, &mut errors);
                    }
                }
                None => errors.push(Error::verify(format!(
                    "alias @{} has no target",
                    module.name(global.name)
                ))),
            }
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
    if !function.is_declaration
        && function.personality.is_none()
        && function.insts.iter().any(|(_, inst)| {
            matches!(
                inst.kind,
                InstKind::LandingPad { .. }
                    | InstKind::CatchSwitch { .. }
                    | InstKind::CatchPad { .. }
                    | InstKind::CleanupPad { .. }
                    | InstKind::CatchRet { .. }
                    | InstKind::CleanupRet { .. }
            )
        })
    {
        errors.push(Error::verify(format!(
            "function @{} with exception-handling instructions requires a personality",
            module.name(function.name)
        )));
    }
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
    let result_ty = inst.result.map(|local| function.local_type(local));
    match &inst.kind {
        InstKind::Binary { lhs, rhs, .. } => {
            check_operand(module, function, *lhs, errors, "operand");
            check_operand(module, function, *rhs, errors, "operand");
        }
        InstKind::ICmp { lhs, rhs, .. } => {
            check_operand(module, function, *lhs, errors, "operand");
            check_operand(module, function, *rhs, errors, "operand");
            if let Some(result_ty) = result_ty
                && !comparison_result_ok(module, lhs.ty, result_ty)
            {
                errors.push(Error::verify(
                    "icmp result type does not match the operand vector",
                ));
            }
        }
        InstKind::FCmp { lhs, rhs, .. } => {
            check_operand(module, function, *lhs, errors, "operand");
            check_operand(module, function, *rhs, errors, "operand");
            if let Some(result_ty) = result_ty
                && !comparison_result_ok(module, lhs.ty, result_ty)
            {
                errors.push(Error::verify(
                    "fcmp result type does not match the operand vector",
                ));
            }
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
        InstKind::Load {
            ptr,
            align,
            ordering,
            ..
        } => {
            check_operand(module, function, *ptr, errors, "load pointer");
            if ordering.is_atomic() {
                if align.is_none() {
                    errors.push(Error::verify("atomic load requires an explicit alignment"));
                }
                if !ordering.is_load_valid() {
                    errors.push(Error::verify("invalid atomic load ordering"));
                }
            }
        }
        InstKind::Store {
            value,
            ptr,
            align,
            ordering,
            ..
        } => {
            check_operand(module, function, *value, errors, "store value");
            check_operand(module, function, *ptr, errors, "store pointer");
            if ordering.is_atomic() {
                if align.is_none() {
                    errors.push(Error::verify("atomic store requires an explicit alignment"));
                }
                if !ordering.is_store_valid() {
                    errors.push(Error::verify("invalid atomic store ordering"));
                }
            }
        }
        InstKind::AtomicRmw {
            op,
            ptr,
            value,
            align,
            ordering,
            ..
        } => {
            check_operand(module, function, *ptr, errors, "atomicrmw pointer");
            check_operand(module, function, *value, errors, "atomicrmw value");
            if align.is_none() {
                errors.push(Error::verify("atomicrmw requires an explicit alignment"));
            }
            if !ordering.is_rmw_valid() {
                errors.push(Error::verify("invalid atomicrmw ordering"));
            }
            if op.is_float() != matches!(module.types.get(value.ty), Type::Float(_)) {
                errors.push(Error::verify(
                    "atomicrmw operation does not match the value type",
                ));
            }
            match result_ty {
                Some(result_ty) if result_ty == value.ty => {}
                Some(_) => errors.push(Error::verify(
                    "atomicrmw result type does not match the value type",
                )),
                None => errors.push(Error::verify("atomicrmw requires a result")),
            }
        }
        InstKind::CmpXchg {
            ptr,
            cmp,
            new,
            align,
            success,
            failure,
            ..
        } => {
            check_operand(module, function, *ptr, errors, "cmpxchg pointer");
            check_operand(module, function, *cmp, errors, "cmpxchg compare value");
            check_operand(module, function, *new, errors, "cmpxchg new value");
            if align.is_none() {
                errors.push(Error::verify("cmpxchg requires an explicit alignment"));
            }
            if !success.is_cmpxchg_success_valid() {
                errors.push(Error::verify("invalid cmpxchg success ordering"));
            }
            if !failure.is_cmpxchg_failure_valid() {
                errors.push(Error::verify("invalid cmpxchg failure ordering"));
            }
            if cmp.ty != new.ty {
                errors.push(Error::verify(
                    "cmpxchg compare and new values have different types",
                ));
            }
            let result_ok =
                result_ty.is_some_and(|result_ty| match module.types.struct_body(result_ty) {
                    Some((fields, false)) if fields.len() == 2 => {
                        fields[0] == cmp.ty && matches!(module.types.get(fields[1]), Type::Int(1))
                    }
                    _ => false,
                });
            if !result_ok {
                errors.push(Error::verify("cmpxchg result type must be { value, i1 }"));
            }
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
        InstKind::CallBr {
            callee,
            args,
            default_dest,
            indirect_dests,
            ..
        } => {
            check_operand(module, function, *callee, errors, "callbr callee");
            for arg in args {
                check_operand(module, function, *arg, errors, "callbr argument");
            }
            check_block(
                function,
                *default_dest,
                errors,
                "callbr default destination",
            );
            for destination in indirect_dests {
                check_block(
                    function,
                    *destination,
                    errors,
                    "callbr indirect destination",
                );
            }
            if indirect_dests.is_empty() {
                errors.push(Error::verify(
                    "callbr requires at least one indirect destination",
                ));
            }
            for (index, destination) in indirect_dests.iter().enumerate() {
                if indirect_dests[..index].contains(destination) {
                    errors.push(Error::verify("callbr has a duplicate indirect destination"));
                }
            }
            match callee.kind {
                OperandKind::Const(id)
                    if module.constants.try_get(id).is_some_and(|constant| {
                        matches!(constant.kind, ConstKind::InlineAsm { .. })
                    }) => {}
                _ => errors.push(Error::verify("callbr callee is not inline asm")),
            }
            for arg in args {
                let OperandKind::Const(id) = arg.kind else {
                    continue;
                };
                let Some(ConstKind::BlockAddress {
                    function: target_function,
                    block,
                }) = module.constants.try_get(id).map(|constant| &constant.kind)
                else {
                    continue;
                };
                let Some(target) = module.functions.try_get(*target_function) else {
                    errors.push(Error::verify(
                        "callbr label argument targets an unknown function",
                    ));
                    continue;
                };
                if target.name != function.name {
                    errors.push(Error::verify(
                        "callbr label argument targets another function",
                    ));
                    continue;
                }
                match function.block_named(*block) {
                    Some(destination) if indirect_dests.contains(&destination) => {}
                    _ => errors.push(Error::verify(
                        "callbr label argument does not match an indirect destination",
                    )),
                }
            }
        }
        InstKind::Invoke {
            callee,
            args,
            normal_dest,
            unwind_dest,
            ..
        } => {
            check_operand(module, function, *callee, errors, "invoke callee");
            for arg in args {
                check_operand(module, function, *arg, errors, "invoke argument");
            }
            check_block(function, *normal_dest, errors, "invoke normal destination");
            check_block(function, *unwind_dest, errors, "invoke unwind destination");
            if let OperandKind::Function(id) = callee.kind
                && let Some(function_ref) = module.functions.try_get(id)
                && module.types.is_valid(function_ref.ty)
                && let Some((_, params, varargs)) = module.function_type(id)
                && !varargs
                && params.len() != args.len()
            {
                errors.push(Error::verify(format!(
                    "invoke of @{} has the wrong number of arguments",
                    module.name(function_ref.name)
                )));
            }
        }
        InstKind::LandingPad { ty, clauses, .. } => {
            if result_ty != Some(*ty) {
                errors.push(Error::verify(
                    "landingpad result type does not match its declared type",
                ));
            }
            for (clause, value) in clauses {
                check_operand(module, function, *value, errors, "landingpad clause");
                match clause {
                    LandingPadClause::Catch
                        if !matches!(module.types.get(value.ty), Type::Pointer) =>
                    {
                        errors.push(Error::verify("landingpad catch clause is not a pointer"));
                    }
                    LandingPadClause::Filter
                        if !matches!(module.types.get(value.ty), Type::Array { .. }) =>
                    {
                        errors.push(Error::verify("landingpad filter clause is not an array"));
                    }
                    _ => {}
                }
            }
        }
        InstKind::Resume { value } => {
            check_operand(module, function, *value, errors, "resume value");
        }
        InstKind::CatchSwitch {
            parent_pad,
            handlers,
            unwind_dest,
        } => {
            if let Some(parent_pad) = parent_pad {
                check_token_operand(
                    module,
                    function,
                    *parent_pad,
                    errors,
                    "catchswitch parent pad",
                );
            }
            if handlers.is_empty() {
                errors.push(Error::verify("catchswitch has no handlers"));
            }
            for handler in handlers {
                check_block(function, *handler, errors, "catchswitch handler");
            }
            if let Some(unwind_dest) = unwind_dest {
                check_block(
                    function,
                    *unwind_dest,
                    errors,
                    "catchswitch unwind destination",
                );
            }
            if let Some(result_ty) = result_ty
                && !matches!(module.types.get(result_ty), Type::Token)
            {
                errors.push(Error::verify("catchswitch result type is not token"));
            }
        }
        InstKind::CatchPad { parent_pad, args } | InstKind::CleanupPad { parent_pad, args } => {
            if let Some(parent_pad) = parent_pad {
                check_token_operand(module, function, *parent_pad, errors, "funclet parent pad");
            }
            for arg in args {
                check_operand(module, function, *arg, errors, "funclet pad argument");
            }
            if let Some(result_ty) = result_ty
                && !matches!(module.types.get(result_ty), Type::Token)
            {
                errors.push(Error::verify("funclet pad result type is not token"));
            }
        }
        InstKind::CatchRet {
            catch_pad,
            successor,
        } => {
            check_token_operand(module, function, *catch_pad, errors, "catchret operand");
            check_block(function, *successor, errors, "catchret successor");
        }
        InstKind::CleanupRet {
            cleanup_pad,
            unwind_dest,
        } => {
            check_token_operand(module, function, *cleanup_pad, errors, "cleanupret operand");
            if let Some(unwind_dest) = unwind_dest {
                check_block(
                    function,
                    *unwind_dest,
                    errors,
                    "cleanupret unwind destination",
                );
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
            if !is_i1_or_vector_i1(module, cond.ty) {
                errors.push(Error::verify(
                    "select condition is not i1 or a vector of i1",
                ));
            }
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
        InstKind::ExtractValue { aggregate, indices } => {
            check_operand(
                module,
                function,
                *aggregate,
                errors,
                "extractvalue aggregate",
            );
            match module.types.aggregate_indexed(aggregate.ty, indices) {
                Some(expected) if result_ty == Some(expected) => {}
                Some(_) => errors.push(Error::verify(
                    "extractvalue result type does not match the indexed element",
                )),
                None => errors.push(Error::verify(
                    "extractvalue indices do not match the aggregate",
                )),
            }
        }
        InstKind::InsertValue {
            aggregate,
            value,
            indices,
        } => {
            check_operand(
                module,
                function,
                *aggregate,
                errors,
                "insertvalue aggregate",
            );
            check_operand(module, function, *value, errors, "insertvalue value");
            match module.types.aggregate_indexed(aggregate.ty, indices) {
                Some(expected) if expected == value.ty => {}
                Some(_) => errors.push(Error::verify(
                    "insertvalue value type does not match the indexed element",
                )),
                None => errors.push(Error::verify(
                    "insertvalue indices do not match the aggregate",
                )),
            }
            if result_ty != Some(aggregate.ty) {
                errors.push(Error::verify(
                    "insertvalue result type does not match the aggregate",
                ));
            }
        }
        InstKind::ExtractElement { vector, index } => {
            check_operand(module, function, *vector, errors, "extractelement vector");
            check_operand(module, function, *index, errors, "extractelement index");
            match module.types.vector_element(vector.ty) {
                Some(expected) if result_ty == Some(expected) => {}
                Some(_) => errors.push(Error::verify(
                    "extractelement result type does not match the vector element",
                )),
                None => errors.push(Error::verify("extractelement operand is not a vector")),
            }
            if !matches!(module.types.get(index.ty), Type::Int(_)) {
                errors.push(Error::verify("extractelement index is not an integer"));
            }
        }
        InstKind::InsertElement {
            vector,
            value,
            index,
        } => {
            check_operand(module, function, *vector, errors, "insertelement vector");
            check_operand(module, function, *value, errors, "insertelement value");
            check_operand(module, function, *index, errors, "insertelement index");
            match module.types.vector_element(vector.ty) {
                Some(element) if element == value.ty => {}
                Some(_) => errors.push(Error::verify(
                    "insertelement value type does not match the vector element",
                )),
                None => errors.push(Error::verify("insertelement operand is not a vector")),
            }
            if !matches!(module.types.get(index.ty), Type::Int(_)) {
                errors.push(Error::verify("insertelement index is not an integer"));
            }
            if result_ty != Some(vector.ty) {
                errors.push(Error::verify(
                    "insertelement result type does not match the vector",
                ));
            }
        }
        InstKind::ShuffleVector {
            first,
            second,
            mask,
        } => {
            check_operand(
                module,
                function,
                *first,
                errors,
                "shufflevector first vector",
            );
            check_operand(
                module,
                function,
                *second,
                errors,
                "shufflevector second vector",
            );
            check_operand(module, function, *mask, errors, "shufflevector mask");
            if !matches!(mask.kind, OperandKind::Const(_)) {
                errors.push(Error::verify("shufflevector mask is not a constant"));
            }
            match module.types.vector_info(first.ty) {
                Some((elem, _, scalable)) => {
                    if scalable {
                        errors.push(Error::verify("scalable shufflevector is not supported"));
                    }
                    if second.ty != first.ty {
                        errors.push(Error::verify(
                            "shufflevector operands must have the same type",
                        ));
                    }
                    match module.types.vector_info(mask.ty) {
                        Some((mask_elem, mask_len, mask_scalable)) => {
                            if mask_scalable {
                                errors.push(Error::verify(
                                    "scalable shufflevector masks are not supported",
                                ));
                            }
                            if !matches!(module.types.get(mask_elem), Type::Int(32)) {
                                errors.push(Error::verify(
                                    "shufflevector mask element type must be i32",
                                ));
                            }
                            if result_ty.and_then(|id| module.types.vector_info(id))
                                != Some((elem, mask_len, false))
                            {
                                errors.push(Error::verify(
                                    "shufflevector result type does not match the mask",
                                ));
                            }
                        }
                        None => errors.push(Error::verify("shufflevector mask is not a vector")),
                    }
                }
                None => errors.push(Error::verify("shufflevector first operand is not a vector")),
            }
        }
        InstKind::Freeze { value } => {
            check_operand(module, function, *value, errors, "freeze value");
            if let Some(result_ty) = result_ty
                && result_ty != value.ty
            {
                errors.push(Error::verify(
                    "freeze result type does not match its operand",
                ));
            }
        }
        InstKind::Fence { ordering, .. } => {
            if !ordering.is_fence_valid() {
                errors.push(Error::verify(
                    "fence ordering must be acquire, release, acq_rel, or seq_cst",
                ));
            }
        }
        InstKind::VaArg { list, ty } => {
            check_operand(module, function, *list, errors, "va_arg list");
            if module.types.is_valid(list.ty) && !matches!(module.types.get(list.ty), Type::Pointer)
            {
                errors.push(Error::verify("va_arg list is not a pointer"));
            }
            if !module.types.is_valid(*ty) {
                errors.push(Error::verify("va_arg result type is invalid"));
            } else if let Some(result_ty) = result_ty
                && result_ty != *ty
            {
                errors.push(Error::verify(
                    "va_arg result type does not match its declared type",
                ));
            }
        }
        InstKind::IndirectBr {
            address,
            destinations,
        } => {
            check_operand(module, function, *address, errors, "indirectbr address");
            if module.types.is_valid(address.ty)
                && !matches!(module.types.get(address.ty), Type::Pointer)
            {
                errors.push(Error::verify("indirectbr address is not a pointer"));
            }
            for destination in destinations {
                check_block(function, *destination, errors, "indirectbr destination");
            }
        }
        InstKind::Unreachable => {}
    }
}

fn comparison_result_ok(
    module: &Module,
    operand_ty: crate::ty::TypeId,
    result_ty: crate::ty::TypeId,
) -> bool {
    match module.types.vector_info(operand_ty) {
        Some((_, len, scalable)) => match module.types.vector_info(result_ty) {
            Some((elem, result_len, result_scalable)) => {
                result_len == len
                    && result_scalable == scalable
                    && matches!(module.types.get(elem), Type::Int(1))
            }
            None => false,
        },
        None => matches!(module.types.get(result_ty), Type::Int(1)),
    }
}

fn is_i1_or_vector_i1(module: &Module, ty: crate::ty::TypeId) -> bool {
    match module.types.get(ty) {
        Type::Int(1) => true,
        Type::Vector { elem, .. } => matches!(module.types.get(*elem), Type::Int(1)),
        _ => false,
    }
}

fn verify_const_expr(
    module: &Module,
    result_ty: crate::ty::TypeId,
    expr: &ConstExpr,
    errors: &mut Vec<VerifyError>,
) {
    match expr {
        ConstExpr::Gep {
            source_ty,
            ptr,
            indices,
            ..
        } => {
            if !module.types.is_valid(result_ty) {
                errors.push(Error::verify(
                    "getelementptr constant expression has an invalid result type",
                ));
            } else if !matches!(module.types.get(result_ty), Type::Pointer) {
                errors.push(Error::verify(
                    "getelementptr constant expression does not produce a pointer",
                ));
            }
            if !module.types.is_valid(*source_ty) {
                errors.push(Error::verify(
                    "getelementptr constant expression has an invalid source type",
                ));
            }
            if !module.types.is_valid(ptr.ty) {
                errors.push(Error::verify(
                    "getelementptr constant expression base has an invalid type",
                ));
            } else if !matches!(module.types.get(ptr.ty), Type::Pointer) {
                errors.push(Error::verify(
                    "getelementptr constant expression base is not a pointer",
                ));
            }
            if indices.is_empty() {
                errors.push(Error::verify(
                    "getelementptr constant expression has no indices",
                ));
            }
            check_const_operand(module, *ptr, errors, "getelementptr constant base");
            for index in indices {
                if !module.types.is_valid(index.ty) {
                    errors.push(Error::verify(
                        "getelementptr constant expression has an invalid index type",
                    ));
                }
                check_const_operand(module, index.value, errors, "getelementptr constant index");
            }
        }
        ConstExpr::Cast { value, .. } => {
            if !module.types.is_valid(result_ty) {
                errors.push(Error::verify(
                    "cast constant expression has an invalid result type",
                ));
            }
            check_const_operand(module, *value, errors, "cast constant operand");
        }
    }
}

fn check_const_operand(
    module: &Module,
    operand: Operand,
    errors: &mut Vec<VerifyError>,
    what: &str,
) {
    match operand.kind {
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
            Some(function) if module.types.is_valid(function.ty) => {}
            Some(_) => errors.push(Error::verify(format!(
                "{what} refers to an undefined function"
            ))),
            None => errors.push(Error::verify(format!("{what} function is out of range"))),
        },
        OperandKind::Local(_) => {
            errors.push(Error::verify(format!("{what} is a local value")));
        }
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

fn verify_alias(
    module: &Module,
    global: &Global,
    aliasee: crate::constant::ConstId,
    errors: &mut Vec<VerifyError>,
) {
    let name = module.name(global.name);
    if aliasee_has_interposable_alias(module, aliasee, 0) {
        errors.push(Error::verify(format!(
            "alias @{name} cannot point to an interposable alias"
        )));
    }
    if !aliasee_is_definition(module, aliasee, 0) {
        errors.push(Error::verify(format!(
            "alias @{name} must point to a definition"
        )));
    }
}

fn aliasee_has_interposable_alias(
    module: &Module,
    id: crate::constant::ConstId,
    depth: u32,
) -> bool {
    if depth > 16 {
        return false;
    }
    match &module.constant(id).kind {
        ConstKind::Global(global) => module.globals.try_get(*global).is_some_and(|global| {
            if global.kind != GlobalKind::Alias {
                return false;
            }
            alias_is_interposable(global)
                || global
                    .init
                    .is_some_and(|init| aliasee_has_interposable_alias(module, init, depth + 1))
        }),
        ConstKind::Expr(ConstExpr::Cast { value, .. }) => match value.kind {
            OperandKind::Const(id) => aliasee_has_interposable_alias(module, id, depth + 1),
            _ => false,
        },
        ConstKind::Expr(ConstExpr::Gep { ptr, .. }) => match ptr.kind {
            OperandKind::Const(id) => aliasee_has_interposable_alias(module, id, depth + 1),
            _ => false,
        },
        _ => false,
    }
}

fn alias_is_interposable(global: &Global) -> bool {
    if matches!(global.linkage, Linkage::Private | Linkage::Internal) {
        return false;
    }
    if matches!(
        global.linkage,
        Linkage::Weak | Linkage::LinkOnce | Linkage::Common | Linkage::ExternWeak
    ) {
        return true;
    }
    !global.dso_local
}

fn verify_ifunc_resolver(
    module: &Module,
    global: &Global,
    resolver: crate::constant::ConstId,
    errors: &mut Vec<VerifyError>,
) {
    let name = module.name(global.name);
    let Some(function) = resolver_function(module, resolver, 0) else {
        errors.push(Error::verify(format!(
            "ifunc @{name} must have a function resolver"
        )));
        return;
    };
    let Some(function) = module.functions.try_get(function) else {
        errors.push(Error::verify(format!(
            "ifunc @{name} resolver is undefined"
        )));
        return;
    };
    if function.is_declaration {
        errors.push(Error::verify(format!(
            "ifunc @{name} resolver must be a definition"
        )));
    }
    let returns_pointer = match module.types.get(function.ty) {
        Type::Function { ret, .. } => matches!(module.types.get(*ret), Type::Pointer),
        _ => false,
    };
    if !returns_pointer {
        errors.push(Error::verify(format!(
            "ifunc @{name} resolver must return a pointer"
        )));
    }
}

fn resolver_function(
    module: &Module,
    id: crate::constant::ConstId,
    depth: u32,
) -> Option<crate::function::FuncId> {
    if depth > 16 {
        return None;
    }
    match &module.constant(id).kind {
        ConstKind::Function(function) => Some(*function),
        ConstKind::Global(global) => module
            .globals
            .try_get(*global)
            .filter(|global| global.kind == GlobalKind::Alias)
            .and_then(|global| global.init)
            .and_then(|init| resolver_function(module, init, depth + 1)),
        ConstKind::Expr(ConstExpr::Cast { value, .. }) => match value.kind {
            OperandKind::Const(id) => resolver_function(module, id, depth + 1),
            _ => None,
        },
        _ => None,
    }
}

fn aliasee_is_definition(module: &Module, id: crate::constant::ConstId, depth: u32) -> bool {
    if depth > 16 {
        return false;
    }
    match &module.constant(id).kind {
        ConstKind::Global(global) => module.globals.try_get(*global).is_some_and(|global| {
            if global.kind.is_alias() {
                global
                    .init
                    .is_some_and(|init| aliasee_is_definition(module, init, depth + 1))
            } else {
                global.init.is_some()
            }
        }),
        ConstKind::Function(function) => module
            .functions
            .try_get(*function)
            .is_some_and(|function| !function.is_declaration),
        ConstKind::Expr(ConstExpr::Cast { value, .. }) => match value.kind {
            OperandKind::Const(id) => aliasee_is_definition(module, id, depth + 1),
            _ => false,
        },
        ConstKind::Expr(ConstExpr::Gep { ptr, .. }) => match ptr.kind {
            OperandKind::Const(id) => aliasee_is_definition(module, id, depth + 1),
            _ => false,
        },
        _ => false,
    }
}

fn verify_block_address(
    module: &Module,
    ty: crate::ty::TypeId,
    function_id: crate::function::FuncId,
    block: llvm_rust_support::Symbol,
    errors: &mut Vec<VerifyError>,
) {
    if !matches!(module.types.get(ty), Type::Pointer) {
        errors.push(Error::verify("blockaddress type is not a pointer"));
    }
    let Some(function) = module.functions.try_get(function_id) else {
        errors.push(Error::verify("blockaddress function is undefined"));
        return;
    };
    let Some(block_id) = function.block_named(block) else {
        errors.push(Error::verify("blockaddress block is undefined"));
        return;
    };
    if !function.block_order().contains(&block_id) {
        errors.push(Error::verify("blockaddress block is not defined"));
    }
}

fn check_token_operand(
    module: &Module,
    function: &Function,
    operand: Operand,
    errors: &mut Vec<VerifyError>,
    what: &str,
) {
    check_operand(module, function, operand, errors, what);
    if !matches!(module.types.get(operand.ty), Type::Token) {
        errors.push(Error::verify(format!("{what} is not token")));
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
