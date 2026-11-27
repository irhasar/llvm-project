//! Deterministic textual LLVM IR printer.

use std::fmt::Write as _;

use crate::constant::{ConstId, ConstKind, Constant};
use crate::function::{BlockId, FuncId, Function};
use crate::inst::{Attribute, FastMath, InstKind, Instruction, IntFlags};
use crate::linkage::{Linkage, Visibility};
use crate::module::{Global, GlobalId, Module};
use crate::names::{quote_name, write_name, write_quoted_bytes};
use crate::ty::{FloatKind, Type, TypeId};
use crate::value::{Operand, OperandKind};

/// Prints a module in canonical textual form.
pub fn print_module(module: &Module) -> String {
    let instruction_count: usize = module
        .functions
        .iter()
        .map(|(_, function)| function.insts.len())
        .sum();
    let mut out = String::with_capacity(
        instruction_count.saturating_mul(48)
            + module.globals.len().saturating_mul(64)
            + module.named_types.len().saturating_mul(32)
            + 4096,
    );
    if let Some(value) = &module.source_filename {
        out.push_str("source_filename = ");
        write_quoted_bytes(&mut out, value.as_bytes());
        out.push('\n');
    }
    if let Some(value) = &module.data_layout {
        out.push_str("target datalayout = ");
        write_quoted_bytes(&mut out, value.as_bytes());
        out.push('\n');
    }
    if let Some(value) = &module.target_triple {
        out.push_str("target triple = ");
        write_quoted_bytes(&mut out, value.as_bytes());
        out.push('\n');
    }
    if module.source_filename.is_some()
        || module.data_layout.is_some()
        || module.target_triple.is_some()
    {
        out.push('\n');
    }

    for &id in &module.named_types {
        if !module.types.is_defined(id) {
            continue;
        }
        let name = module.types.name(id).unwrap_or("anonymous");
        if module.types.is_opaque(id) {
            let _ = writeln!(out, "%{} = type opaque", quote_name(name));
            continue;
        }
        let (fields, packed) = module.types.struct_body(id).unwrap_or((&[], false));
        let _ = writeln!(
            out,
            "%{} = type {}",
            quote_name(name),
            struct_body(module, fields, packed)
        );
    }
    if !module.named_types.is_empty() {
        out.push('\n');
    }

    for id in module.ordered_globals() {
        print_global(module, id, &module.globals[id], &mut out);
    }
    if !module.globals.is_empty() {
        out.push('\n');
    }

    let mut first_function = true;
    for id in module.ordered_functions() {
        let function = &module.functions[id];
        if !first_function {
            out.push('\n');
        }
        first_function = false;
        print_function(module, id, function, &mut out);
    }

    if !module.attr_groups.is_empty() {
        out.push('\n');
        for (group, text) in &module.attr_groups {
            let _ = writeln!(out, "attributes #{group} = {{ {text} }}");
        }
    }
    out
}

fn struct_body(module: &Module, fields: &[TypeId], packed: bool) -> String {
    let mut out = String::new();
    out.push_str(if packed { "<{ " } else { "{ " });
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}", module.display_type(*field));
    }
    out.push_str(if packed { " }>" } else { " }" });
    out
}

fn write_declared_name(out: &mut String, name: &str) {
    if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()) {
        out.push('"');
        out.push_str(name);
        out.push('"');
    } else {
        let _ = write_name(out, name);
    }
}

fn write_global_name(out: &mut String, module: &Module, id: GlobalId) {
    if module.is_unnamed_global(id) {
        if let Some(slot) = module.unnamed_global_slot(id) {
            let _ = write!(out, "{slot}");
        } else {
            out.push_str(module.name(module.globals[id].name));
        }
        return;
    }
    write_declared_name(out, module.name(module.globals[id].name));
}

fn write_function_name(out: &mut String, module: &Module, id: FuncId) {
    if module.is_unnamed_function(id) {
        if let Some(slot) = module.unnamed_function_slot(id) {
            let _ = write!(out, "{slot}");
        } else {
            out.push_str(module.name(module.functions[id].name));
        }
        return;
    }
    write_declared_name(out, module.name(module.functions[id].name));
}

fn print_global(module: &Module, id: GlobalId, global: &Global, out: &mut String) {
    out.push('@');
    write_global_name(out, module, id);
    out.push_str(" = ");
    if global.linkage != Linkage::External {
        let _ = write!(out, "{} ", global.linkage.as_str());
    }
    if global.visibility != Visibility::Default {
        let _ = write!(out, "{} ", global.visibility.as_str());
    }
    if global.dso_local {
        out.push_str("dso_local ");
    }
    if global.tls_model.is_thread_local() {
        let _ = write!(out, "{} ", global.tls_model.as_str());
    }
    if global.local_unnamed_addr {
        out.push_str("local_unnamed_addr ");
    } else if global.unnamed_addr {
        out.push_str("unnamed_addr ");
    }
    if global.init.is_none() && global.linkage == Linkage::External {
        out.push_str("external ");
    }
    out.push_str(if global.is_const {
        "constant "
    } else {
        "global "
    });
    let _ = write!(out, "{}", module.display_type(global.ty));
    if let Some(init) = global.init {
        out.push(' ');
        write_constant(module, init, out);
    }
    if let Some(align) = global.align {
        let _ = write!(out, ", align {align}");
    }
    if let Some(section) = &global.section {
        out.push_str(", section ");
        write_quoted_bytes(out, section.as_bytes());
    }
    out.push('\n');
}

fn print_function(module: &Module, id: FuncId, function: &Function, out: &mut String) {
    let Some((ret, params, varargs)) = module.function_type(id) else {
        return;
    };
    out.push_str(if function.is_declaration {
        "declare "
    } else {
        "define "
    });
    if function.linkage != Linkage::External {
        let _ = write!(out, "{} ", function.linkage.as_str());
    }
    if function.visibility != Visibility::Default {
        let _ = write!(out, "{} ", function.visibility.as_str());
    }
    if function.dso_local {
        out.push_str("dso_local ");
    }
    for attr in function.ret_attrs.iter() {
        let _ = write!(out, "{attr} ");
    }
    let _ = write!(out, "{} @", module.display_type(ret));
    write_function_name(out, module, id);
    out.push('(');
    for (index, param) in function.params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}", module.display_type(param.ty));
        for attr in param.attrs.iter() {
            let _ = write!(out, " {attr}");
        }
        if let Some(name) = param.name {
            out.push_str(" %");
            let _ = write_name(out, module.name(name));
        }
    }
    if varargs {
        if !params.is_empty() {
            out.push_str(", ");
        }
        out.push_str("...");
    }
    out.push(')');
    if function.local_unnamed_addr {
        out.push_str(" local_unnamed_addr");
    } else if function.unnamed_addr {
        out.push_str(" unnamed_addr");
    }
    if let Some(align) = function.align {
        let _ = write!(out, " align {align}");
    }
    for attr in function.attrs.iter() {
        let _ = write!(out, " {attr}");
    }
    if function.is_declaration {
        out.push('\n');
        return;
    }
    out.push_str(" {\n");
    for (block_id, block) in function.blocks_in_order() {
        let label = block.name.map(|name| module.name(name)).unwrap_or("0");
        let _ = write_name(out, label);
        out.push_str(":\n");
        for &inst_id in &block.insts {
            out.push_str("  ");
            let inst = &function.insts[inst_id];
            if let Some(result) = inst.result {
                let name = function
                    .local_name(result)
                    .map(|name| module.name(name))
                    .unwrap_or("0");
                out.push('%');
                let _ = write_name(out, name);
                out.push_str(" = ");
            }
            write_instruction(module, function, inst, block_id, out);
            out.push('\n');
        }
    }
    out.push_str("}\n");
}

fn write_instruction(
    module: &Module,
    function: &Function,
    inst: &Instruction,
    _block: BlockId,
    out: &mut String,
) {
    match &inst.kind {
        InstKind::Binary {
            op,
            flags,
            fastmath,
            lhs,
            rhs,
        } => {
            let _ = write!(out, "{}", op.as_str());
            write_int_flags(*flags, out);
            write_fastmath(*fastmath, out);
            out.push(' ');
            write_operand(module, function, *lhs, out);
            out.push_str(", ");
            write_value(module, function, *rhs, out);
        }
        InstKind::FNeg { flags, value } => {
            out.push_str("fneg");
            write_fastmath(*flags, out);
            out.push(' ');
            write_operand(module, function, *value, out);
        }
        InstKind::ICmp { pred, lhs, rhs } => {
            let _ = write!(out, "icmp {} ", pred.as_str());
            write_operand(module, function, *lhs, out);
            out.push_str(", ");
            write_value(module, function, *rhs, out);
        }
        InstKind::FCmp {
            pred,
            flags,
            lhs,
            rhs,
        } => {
            out.push_str("fcmp");
            write_fastmath(*flags, out);
            let _ = write!(out, " {} ", pred.as_str());
            write_operand(module, function, *lhs, out);
            out.push_str(", ");
            write_value(module, function, *rhs, out);
        }
        InstKind::Alloca {
            elem_ty,
            count,
            align,
        } => {
            let _ = write!(out, "alloca {}", module.display_type(*elem_ty));
            if let Some(count) = count {
                out.push_str(", ");
                write_operand(module, function, *count, out);
            }
            if let Some(align) = align {
                let _ = write!(out, ", align {align}");
            }
        }
        InstKind::Load {
            ty,
            ptr,
            align,
            volatile,
        } => {
            out.push_str("load ");
            if *volatile {
                out.push_str("volatile ");
            }
            let _ = write!(out, "{}, ", module.display_type(*ty));
            write_operand(module, function, *ptr, out);
            if let Some(align) = align {
                let _ = write!(out, ", align {align}");
            }
        }
        InstKind::Store {
            value,
            ptr,
            align,
            volatile,
        } => {
            out.push_str("store ");
            if *volatile {
                out.push_str("volatile ");
            }
            write_operand(module, function, *value, out);
            out.push_str(", ");
            write_operand(module, function, *ptr, out);
            if let Some(align) = align {
                let _ = write!(out, ", align {align}");
            }
        }
        InstKind::Gep {
            inbounds,
            source_ty,
            ptr,
            indices,
        } => {
            out.push_str("getelementptr ");
            if *inbounds {
                out.push_str("inbounds ");
            }
            let _ = write!(out, "{}, ", module.display_type(*source_ty));
            write_operand(module, function, *ptr, out);
            for index in indices {
                out.push_str(", ");
                write_operand(module, function, index.value, out);
            }
        }
        InstKind::Call {
            tail,
            ret_ty,
            fn_ty,
            callee,
            args,
            arg_attrs,
            fastmath,
            ret_attrs,
            attrs,
        } => {
            out.push_str(tail.prefix());
            out.push_str("call");
            write_fastmath(*fastmath, out);
            out.push(' ');
            for attr in attrs.iter().filter(|attr| is_calling_convention(attr)) {
                let _ = write!(out, "{attr} ");
            }
            for attr in ret_attrs.iter() {
                let _ = write!(out, "{attr} ");
            }
            let ty = match module.types.get(*fn_ty) {
                Type::Function { varargs: true, .. } => *fn_ty,
                _ => *ret_ty,
            };
            let _ = write!(out, "{} ", module.display_type(ty));
            write_callee(module, function, *callee, out);
            out.push('(');
            for (index, arg) in args.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{} ", module.display_type(arg.ty));
                if let Some(attrs) = arg_attrs.get(index) {
                    for attr in attrs.iter() {
                        let _ = write!(out, "{attr} ");
                    }
                }
                write_value(module, function, *arg, out);
            }
            out.push(')');
            for attr in attrs.iter().filter(|attr| !is_calling_convention(attr)) {
                let _ = write!(out, " {attr}");
            }
        }
        InstKind::Phi { incoming } => {
            out.push_str("phi ");
            if let Some((first, _)) = incoming.first() {
                let _ = write!(out, "{} ", module.display_type(first.ty));
            }
            for (index, (value, block)) in incoming.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str("[ ");
                write_value(module, function, *value, out);
                let label = function
                    .blocks
                    .get(*block)
                    .name
                    .map(|name| module.name(name))
                    .unwrap_or("0");
                out.push_str(", %");
                let _ = write_name(out, label);
                out.push_str(" ]");
            }
        }
        InstKind::Select {
            cond,
            on_true,
            on_false,
            fastmath,
        } => {
            out.push_str("select");
            write_fastmath(*fastmath, out);
            out.push(' ');
            write_operand(module, function, *cond, out);
            out.push_str(", ");
            write_operand(module, function, *on_true, out);
            out.push_str(", ");
            write_operand(module, function, *on_false, out);
        }
        InstKind::Cast { op, value, to } => {
            let _ = write!(out, "{} ", op.as_str());
            write_operand(module, function, *value, out);
            let _ = write!(out, " to {}", module.display_type(*to));
        }
        InstKind::Ret { value } => {
            out.push_str("ret ");
            if let Some(value) = value {
                write_operand(module, function, *value, out);
            } else {
                out.push_str("void");
            }
        }
        InstKind::Br { dest } => {
            let label = function
                .blocks
                .get(*dest)
                .name
                .map(|name| module.name(name))
                .unwrap_or("0");
            out.push_str("br label %");
            let _ = write_name(out, label);
        }
        InstKind::CondBr {
            cond,
            then_block,
            else_block,
        } => {
            out.push_str("br ");
            write_operand(module, function, *cond, out);
            out.push_str(", ");
            let then_label = function
                .blocks
                .get(*then_block)
                .name
                .map(|name| module.name(name))
                .unwrap_or("0");
            let else_label = function
                .blocks
                .get(*else_block)
                .name
                .map(|name| module.name(name))
                .unwrap_or("0");
            out.push_str("label %");
            let _ = write_name(out, then_label);
            out.push_str(", label %");
            let _ = write_name(out, else_label);
        }
        InstKind::Switch {
            value,
            default,
            cases,
        } => {
            out.push_str("switch ");
            write_operand(module, function, *value, out);
            let default_label = function
                .blocks
                .get(*default)
                .name
                .map(|name| module.name(name))
                .unwrap_or("0");
            out.push_str(", label %");
            let _ = write_name(out, default_label);
            out.push_str(" [");
            if !cases.is_empty() {
                out.push(' ');
            }
            for (index, (case, block)) in cases.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                let constant = module.constant(*case);
                let _ = write!(out, "{} ", module.display_type(constant.ty));
                write_constant(module, *case, out);
                let label = function
                    .blocks
                    .get(*block)
                    .name
                    .map(|name| module.name(name))
                    .unwrap_or("0");
                out.push_str(", label %");
                let _ = write_name(out, label);
            }
            if !cases.is_empty() {
                out.push(' ');
            }
            out.push(']');
        }
        InstKind::ExtractValue { aggregate, indices } => {
            out.push_str("extractvalue ");
            write_operand(module, function, *aggregate, out);
            for index in indices {
                let _ = write!(out, ", {index}");
            }
        }
        InstKind::InsertValue {
            aggregate,
            value,
            indices,
        } => {
            out.push_str("insertvalue ");
            write_operand(module, function, *aggregate, out);
            out.push_str(", ");
            write_operand(module, function, *value, out);
            for index in indices {
                let _ = write!(out, ", {index}");
            }
        }
        InstKind::ExtractElement { vector, index } => {
            out.push_str("extractelement ");
            write_operand(module, function, *vector, out);
            out.push_str(", ");
            write_operand(module, function, *index, out);
        }
        InstKind::InsertElement {
            vector,
            value,
            index,
        } => {
            out.push_str("insertelement ");
            write_operand(module, function, *vector, out);
            out.push_str(", ");
            write_operand(module, function, *value, out);
            out.push_str(", ");
            write_operand(module, function, *index, out);
        }
        InstKind::ShuffleVector {
            first,
            second,
            mask,
        } => {
            out.push_str("shufflevector ");
            write_operand(module, function, *first, out);
            out.push_str(", ");
            write_operand(module, function, *second, out);
            out.push_str(", ");
            write_operand(module, function, *mask, out);
        }
        InstKind::Freeze { value } => {
            out.push_str("freeze ");
            write_operand(module, function, *value, out);
        }
        InstKind::Fence {
            ordering,
            syncscope,
        } => {
            out.push_str("fence ");
            if *syncscope == crate::inst::SyncScope::SingleThread {
                out.push_str("syncscope(\"singlethread\") ");
            }
            out.push_str(ordering.as_str());
        }
        InstKind::Unreachable => out.push_str("unreachable"),
    }
}

fn write_callee(module: &Module, function: &Function, operand: Operand, out: &mut String) {
    match operand.kind {
        OperandKind::Function(id) => {
            out.push('@');
            write_function_name(out, module, id);
        }
        OperandKind::Global(id) => {
            out.push('@');
            write_global_name(out, module, id);
        }
        OperandKind::Local(id) => {
            let name = function
                .local_name(id)
                .map(|name| module.name(name))
                .unwrap_or("0");
            out.push('%');
            let _ = write_name(out, name);
        }
        OperandKind::Const(id) => write_constant(module, id, out),
    }
}

fn write_operand(module: &Module, function: &Function, operand: Operand, out: &mut String) {
    let _ = write!(out, "{} ", module.display_type(operand.ty));
    write_value(module, function, operand, out);
}

fn write_value(module: &Module, function: &Function, operand: Operand, out: &mut String) {
    match operand.kind {
        OperandKind::Local(id) => {
            let name = function
                .local_name(id)
                .map(|name| module.name(name))
                .unwrap_or("0");
            out.push('%');
            let _ = write_name(out, name);
        }
        OperandKind::Const(id) => write_constant(module, id, out),
        OperandKind::Global(id) => {
            out.push('@');
            write_global_name(out, module, id);
        }
        OperandKind::Function(id) => {
            out.push('@');
            write_function_name(out, module, id);
        }
    }
}

fn write_constant(module: &Module, id: ConstId, out: &mut String) {
    let constant: &Constant = module.constant(id);
    match &constant.kind {
        ConstKind::Int(value) => {
            let bits = module.types.get(constant.ty).int_bits().unwrap_or(32);
            if bits == 1 {
                out.push_str(if *value & 1 == 1 { "true" } else { "false" });
            } else {
                let masked = if bits >= 128 {
                    *value
                } else {
                    *value & ((1u128 << bits) - 1)
                };
                let signed = if bits >= 128 {
                    masked as i128
                } else {
                    let shift = 128 - bits;
                    ((masked << shift) as i128) >> shift
                };
                write_i128(out, signed);
            }
        }
        ConstKind::Float(bits) => write_float(module, constant.ty, *bits, out),
        ConstKind::Null => out.push_str("null"),
        ConstKind::Undef => out.push_str("undef"),
        ConstKind::Poison => out.push_str("poison"),
        ConstKind::ZeroInitializer => out.push_str("zeroinitializer"),
        ConstKind::String(bytes) => {
            out.push('c');
            write_quoted_bytes(out, bytes);
        }
        ConstKind::MetadataString(text) => {
            out.push('!');
            write_quoted_bytes(out, text.as_bytes());
        }
        ConstKind::Global(id) => {
            out.push('@');
            write_global_name(out, module, *id);
        }
        ConstKind::Function(id) => {
            out.push('@');
            write_function_name(out, module, *id);
        }
        ConstKind::Aggregate(elems) => {
            let (open, close, padded) = match module.types.get(constant.ty) {
                Type::Array { .. } => ("[", "]", false),
                Type::Vector { .. } => ("<", ">", false),
                Type::Struct { packed: true, .. } => ("<{", "}>", true),
                Type::Struct { .. } => ("{", "}", true),
                _ => ("[", "]", false),
            };
            out.push_str(open);
            if padded && !elems.is_empty() {
                out.push(' ');
            }
            for (index, elem) in elems.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                let elem_constant = module.constant(*elem);
                let _ = write!(out, "{} ", module.display_type(elem_constant.ty));
                write_constant(module, *elem, out);
            }
            if padded && !elems.is_empty() {
                out.push(' ');
            }
            out.push_str(close);
        }
    }
}

fn write_i128(out: &mut String, value: i128) {
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    let mut buffer = [0u8; 40];
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }
    if negative {
        cursor -= 1;
        buffer[cursor] = b'-';
    }
    out.push_str(core::str::from_utf8(&buffer[cursor..]).expect("decimal digits"));
}

fn write_float(module: &Module, ty: TypeId, bits: u64, out: &mut String) {
    let kind = match module.types.get(ty) {
        Type::Float(kind) => *kind,
        _ => FloatKind::Double,
    };
    match kind {
        FloatKind::Half => {
            let _ = write!(out, "0xH{:04X}", bits & 0xffff);
        }
        FloatKind::BFloat => {
            let _ = write!(out, "0xR{:04X}", bits & 0xffff);
        }
        FloatKind::Float => {
            let _ = write!(out, "0x{:016X}", f32_bits_to_f64_bits(bits as u32));
        }
        FloatKind::Double => {
            let _ = write!(out, "0x{:016X}", bits);
        }
        FloatKind::X86Fp80 => {
            let _ = write!(out, "0xK{:020X}", bits);
        }
        FloatKind::Fp128 => {
            let _ = write!(out, "0xL{:032X}", bits);
        }
        FloatKind::PpcFp128 => {
            let _ = write!(out, "0xM{:032X}", bits);
        }
    }
}

fn f32_bits_to_f64_bits(bits: u32) -> u64 {
    let sign = ((bits >> 31) as u64) << 63;
    let exp = ((bits >> 23) & 0xff) as u64;
    let frac = (bits & 0x007f_ffff) as u64;
    if exp == 0xff {
        if frac == 0 {
            return sign | 0x7ff0_0000_0000_0000;
        }
        let payload = (frac << 29).max(1);
        return sign | 0x7ff0_0000_0000_0000 | payload;
    }
    (f32::from_bits(bits) as f64).to_bits()
}

fn write_int_flags(flags: IntFlags, out: &mut String) {
    if flags.nsw {
        out.push_str(" nsw");
    }
    if flags.nuw {
        out.push_str(" nuw");
    }
    if flags.exact {
        out.push_str(" exact");
    }
}

fn write_fastmath(flags: FastMath, out: &mut String) {
    if flags.fast {
        out.push_str(" fast");
    }
    if flags.nnan {
        out.push_str(" nnan");
    }
    if flags.ninf {
        out.push_str(" ninf");
    }
    if flags.nsz {
        out.push_str(" nsz");
    }
    if flags.arcp {
        out.push_str(" arcp");
    }
    if flags.contract {
        out.push_str(" contract");
    }
    if flags.afn {
        out.push_str(" afn");
    }
    if flags.reassoc {
        out.push_str(" reassoc");
    }
}

fn is_calling_convention(attr: &Attribute) -> bool {
    attr.0.starts_with("cc ")
        || matches!(
            attr.0.as_str(),
            "fastcc" | "coldcc" | "swiftcc" | "tailcc" | "preserve_mostcc" | "preserve_allcc"
        )
}
