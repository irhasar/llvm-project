//! LLVM bitcode writer for the Track B IR subset.

use std::collections::HashMap;

use llvm_rust_ir::{
    BinOp, BlockId, CastOp, ConstId, ConstKind, FastMath, FcmpPred, FuncId, Function, GlobalId,
    IcmpPred, InstKind, IntFlags, Linkage, Module, Operand, OperandKind, TailKind, Type, TypeId,
    Visibility,
};
use llvm_rust_support::{Error, Result};

use crate::bitstream::{AbbrevOp, BitWriter};

const MODULE_BLOCK: u32 = 8;
const CONSTANTS_BLOCK: u32 = 11;
const FUNCTION_BLOCK: u32 = 12;
const IDENTIFICATION_BLOCK: u32 = 13;
const VALUE_SYMTAB_BLOCK: u32 = 14;
const TYPE_BLOCK: u32 = 17;
const STRTAB_BLOCK: u32 = 23;

const IDENTIFICATION_STRING: u32 = 1;
const IDENTIFICATION_EPOCH: u32 = 2;

const MODULE_VERSION: u32 = 1;
const MODULE_TRIPLE: u32 = 2;
const MODULE_DATALAYOUT: u32 = 3;
const MODULE_GLOBALVAR: u32 = 7;
const MODULE_FUNCTION: u32 = 8;
const MODULE_SOURCE_FILENAME: u32 = 16;

const TYPE_NUMENTRY: u32 = 1;
const TYPE_VOID: u32 = 2;
const TYPE_FLOAT: u32 = 3;
const TYPE_DOUBLE: u32 = 4;
const TYPE_LABEL: u32 = 5;
const TYPE_OPAQUE: u32 = 6;
const TYPE_INTEGER: u32 = 7;
const TYPE_HALF: u32 = 10;
const TYPE_ARRAY: u32 = 11;
const TYPE_VECTOR: u32 = 12;
const TYPE_X86_FP80: u32 = 13;
const TYPE_FP128: u32 = 14;
const TYPE_PPC_FP128: u32 = 15;
const TYPE_METADATA: u32 = 16;
const TYPE_STRUCT_ANON: u32 = 18;
const TYPE_STRUCT_NAME: u32 = 19;
const TYPE_STRUCT_NAMED: u32 = 20;
const TYPE_FUNCTION: u32 = 21;
const TYPE_TOKEN: u32 = 22;
const TYPE_BFLOAT: u32 = 23;
const TYPE_OPAQUE_POINTER: u32 = 25;

const CST_SETTYPE: u32 = 1;
const CST_NULL: u32 = 2;
const CST_UNDEF: u32 = 3;
const CST_INTEGER: u32 = 4;
const CST_FLOAT: u32 = 6;
const CST_AGGREGATE: u32 = 7;
const CST_STRING: u32 = 8;
const CST_POISON: u32 = 26;

const FUNC_DECLAREBLOCKS: u32 = 1;
const FUNC_INST_BINOP: u32 = 2;
const FUNC_INST_CAST: u32 = 3;
const FUNC_INST_RET: u32 = 10;
const FUNC_INST_BR: u32 = 11;
const FUNC_INST_SWITCH: u32 = 12;
const FUNC_INST_UNREACHABLE: u32 = 15;
const FUNC_INST_PHI: u32 = 16;
const FUNC_INST_ALLOCA: u32 = 19;
const FUNC_INST_LOAD: u32 = 20;
const FUNC_INST_CMP2: u32 = 28;
const FUNC_INST_VSELECT: u32 = 29;
const FUNC_INST_CALL: u32 = 34;
const FUNC_INST_GEP: u32 = 43;
const FUNC_INST_STORE: u32 = 44;
const FUNC_INST_UNOP: u32 = 56;

const VST_ENTRY: u32 = 1;
const VST_BBENTRY: u32 = 2;
const STRTAB_BLOB: u32 = 1;

/// Serializes a module to LLVM bitcode.
pub fn write_bitcode(module: &mut Module) -> Result<Vec<u8>> {
    ensure_implicit_alloca(module);
    let mut writer = Writer::new(module);
    writer.write()?;
    Ok(writer.finish())
}

fn ensure_implicit_alloca(module: &mut Module) {
    let needs_size = module.functions.iter().any(|(_, function)| {
        function
            .insts
            .iter()
            .any(|(_, inst)| matches!(inst.kind, InstKind::Alloca { count: None, .. }))
    });
    if !needs_size {
        return;
    }
    let i32_ty = module.types.int(32);
    let exists = module
        .constants
        .iter()
        .any(|(_, constant)| constant.ty == i32_ty && constant.kind == ConstKind::Int(1));
    if !exists {
        module.const_int(i32_ty, 1);
    }
}

struct Writer<'a> {
    module: &'a Module,
    stream: BitWriter,
    type_ids: HashMap<TypeId, u32>,
    constants: Vec<ConstId>,
    const_ids: HashMap<ConstId, u32>,
    globals: HashMap<GlobalId, u32>,
    functions: HashMap<FuncId, u32>,
    base_local: u32,
    strtab: Vec<u8>,
    str_offsets: HashMap<String, u32>,
    i32_one: Option<ConstId>,
}

impl<'a> Writer<'a> {
    fn new(module: &'a Module) -> Self {
        let mut type_ids = HashMap::new();
        for (index, (id, _)) in module.types.iter().enumerate() {
            type_ids.insert(id, index as u32);
        }
        let mut globals = HashMap::new();
        for (index, (id, _)) in module.globals.iter().enumerate() {
            globals.insert(id, index as u32);
        }
        let mut functions = HashMap::new();
        for (index, (id, _)) in module.functions.iter().enumerate() {
            functions.insert(id, globals.len() as u32 + index as u32);
        }
        let mut constants = Vec::new();
        for (id, constant) in module.constants.iter() {
            if !matches!(constant.kind, ConstKind::Global(_) | ConstKind::Function(_)) {
                constants.push(id);
            }
        }
        let mut const_ids = HashMap::new();
        let base_const = globals.len() as u32 + functions.len() as u32;
        for (index, id) in constants.iter().enumerate() {
            const_ids.insert(*id, base_const + index as u32);
        }
        let base_local = base_const + constants.len() as u32;
        let i32_ty = module.types.iter().find_map(|(id, ty)| match ty {
            Type::Int(32) => Some(id),
            _ => None,
        });
        let i32_one = i32_ty.and_then(|ty| {
            module
                .constants
                .iter()
                .find(|(_, constant)| constant.ty == ty && constant.kind == ConstKind::Int(1))
                .map(|(id, _)| id)
        });
        let mut writer = Self {
            module,
            stream: BitWriter::new(),
            type_ids,
            constants,
            const_ids,
            globals,
            functions,
            base_local,
            strtab: Vec::new(),
            str_offsets: HashMap::new(),
            i32_one,
        };
        for (_, global) in module.globals.iter() {
            writer.add_string(module.name(global.name));
        }
        for (_, function) in module.functions.iter() {
            writer.add_string(module.name(function.name));
        }
        writer
    }

    fn add_string(&mut self, text: &str) -> u32 {
        if let Some(offset) = self.str_offsets.get(text) {
            return *offset;
        }
        let offset = self.strtab.len() as u32;
        self.strtab.extend_from_slice(text.as_bytes());
        self.strtab.push(0);
        self.str_offsets.insert(text.to_string(), offset);
        offset
    }

    fn write(&mut self) -> Result<()> {
        self.write_identification();
        self.stream.enter_subblock(MODULE_BLOCK, 3);
        self.stream.emit_record(MODULE_VERSION, &[2]);
        self.write_types()?;
        if let Some(triple) = &self.module.target_triple {
            let operands: Vec<u64> = triple.bytes().map(|b| b as u64).collect();
            self.stream.emit_record(MODULE_TRIPLE, &operands);
        }
        if let Some(layout) = &self.module.data_layout {
            let operands: Vec<u64> = layout.bytes().map(|b| b as u64).collect();
            self.stream.emit_record(MODULE_DATALAYOUT, &operands);
        }
        if let Some(filename) = &self.module.source_filename {
            let operands: Vec<u64> = filename.bytes().map(|b| b as u64).collect();
            self.stream.emit_record(MODULE_SOURCE_FILENAME, &operands);
        }
        self.write_globals()?;
        self.write_functions()?;
        self.write_constants()?;
        self.write_bodies()?;
        self.stream.exit_block();
        self.write_strtab();
        Ok(())
    }

    fn write_identification(&mut self) {
        self.stream.enter_subblock(IDENTIFICATION_BLOCK, 5);
        let producer: Vec<u64> = b"llvm-rust 0.1.0".iter().map(|b| *b as u64).collect();
        self.stream.emit_record(IDENTIFICATION_STRING, &producer);
        self.stream.emit_record(IDENTIFICATION_EPOCH, &[0]);
        self.stream.exit_block();
    }

    fn write_types(&mut self) -> Result<()> {
        self.stream.enter_subblock(TYPE_BLOCK, 4);
        self.stream
            .emit_record(TYPE_NUMENTRY, &[self.type_ids.len() as u64]);
        let types: Vec<(TypeId, Type)> = self
            .module
            .types
            .iter()
            .map(|(id, ty)| (id, ty.clone()))
            .collect();
        for (id, ty) in types {
            self.write_type(id, &ty)?;
        }
        self.stream.exit_block();
        Ok(())
    }

    fn write_type(&mut self, id: TypeId, ty: &Type) -> Result<()> {
        match ty {
            Type::Void => self.stream.emit_record(TYPE_VOID, &[]),
            Type::Label => self.stream.emit_record(TYPE_LABEL, &[]),
            Type::Token => self.stream.emit_record(TYPE_TOKEN, &[]),
            Type::Metadata => self.stream.emit_record(TYPE_METADATA, &[]),
            Type::Opaque => self.stream.emit_record(TYPE_OPAQUE, &[]),
            Type::Int(bits) => self.stream.emit_record(TYPE_INTEGER, &[*bits as u64]),
            Type::Float(kind) => {
                let code = match kind {
                    llvm_rust_ir::FloatKind::Half => TYPE_HALF,
                    llvm_rust_ir::FloatKind::BFloat => TYPE_BFLOAT,
                    llvm_rust_ir::FloatKind::Float => TYPE_FLOAT,
                    llvm_rust_ir::FloatKind::Double => TYPE_DOUBLE,
                    llvm_rust_ir::FloatKind::X86Fp80 => TYPE_X86_FP80,
                    llvm_rust_ir::FloatKind::Fp128 => TYPE_FP128,
                    llvm_rust_ir::FloatKind::PpcFp128 => TYPE_PPC_FP128,
                };
                self.stream.emit_record(code, &[]);
            }
            Type::Pointer => {
                self.stream.emit_record(TYPE_OPAQUE_POINTER, &[0]);
            }
            Type::Array { elem, len } => {
                let elem = self.type_index(*elem)?;
                self.stream.emit_record(TYPE_ARRAY, &[*len, elem as u64]);
            }
            Type::Vector {
                elem,
                len,
                scalable,
            } => {
                let elem = self.type_index(*elem)?;
                let mut operands = vec![*len as u64, elem as u64];
                if *scalable {
                    operands.push(1);
                }
                self.stream.emit_record(TYPE_VECTOR, &operands);
            }
            Type::Struct {
                fields,
                packed,
                name,
            } => {
                if let Some(name) = name {
                    if !self.module.types.is_defined(id) {
                        self.stream.emit_record(TYPE_OPAQUE, &[]);
                        return Ok(());
                    }
                    let name_operands: Vec<u64> = name.bytes().map(|b| b as u64).collect();
                    self.stream.emit_record(TYPE_STRUCT_NAME, &name_operands);
                    let mut operands = vec![*packed as u64];
                    for field in fields {
                        operands.push(self.type_index(*field)? as u64);
                    }
                    self.stream.emit_record(TYPE_STRUCT_NAMED, &operands);
                } else {
                    let mut operands = vec![*packed as u64];
                    for field in fields {
                        operands.push(self.type_index(*field)? as u64);
                    }
                    self.stream.emit_record(TYPE_STRUCT_ANON, &operands);
                }
            }
            Type::Function {
                ret,
                params,
                varargs,
            } => {
                let mut operands = vec![*varargs as u64, self.type_index(*ret)? as u64];
                for param in params {
                    operands.push(self.type_index(*param)? as u64);
                }
                self.stream.emit_record(TYPE_FUNCTION, &operands);
            }
        }
        Ok(())
    }

    fn write_globals(&mut self) -> Result<()> {
        let globals: Vec<(GlobalId, llvm_rust_ir::Global)> = self
            .module
            .globals
            .iter()
            .map(|(id, global)| (id, global.clone()))
            .collect();
        for (_, global) in globals {
            let name = self.module.name(global.name);
            let name_offset = self.add_string(name) as u64;
            let type_id = self.type_index(global.ty)? as u64;
            let flags = 2 | global.is_const as u64;
            let init = match global.init {
                Some(id) => self.const_value_id(id)? as u64 + 1,
                None => 0,
            };
            let operands = vec![
                name_offset,
                name.len() as u64,
                type_id,
                flags,
                init,
                linkage_code(global.linkage) as u64,
                encode_align(global.align) as u64,
                0,
                visibility_code(global.visibility) as u64,
                global.thread_local as u64,
                global.unnamed_addr as u64,
                0,
                0,
                0,
                0,
                global.dso_local as u64,
                0,
                0,
                0,
                0,
            ];
            self.stream.emit_record(MODULE_GLOBALVAR, &operands);
        }
        Ok(())
    }

    fn write_functions(&mut self) -> Result<()> {
        let functions: Vec<(FuncId, Function)> = self
            .module
            .functions
            .iter()
            .map(|(id, function)| (id, function.clone()))
            .collect();
        for (_, function) in functions {
            let name = self.module.name(function.name);
            let name_offset = self.add_string(name) as u64;
            let type_id = self.type_index(function.ty)? as u64;
            let operands = vec![
                name_offset,
                name.len() as u64,
                type_id,
                0,
                function.is_declaration as u64,
                linkage_code(function.linkage) as u64,
                0,
                0,
                0,
                visibility_code(function.visibility) as u64,
                0,
                function.unnamed_addr as u64,
                0,
                0,
                0,
                0,
                0,
                function.dso_local as u64,
                0,
                0,
                0,
                0,
            ];
            self.stream.emit_record(MODULE_FUNCTION, &operands);
        }
        Ok(())
    }

    fn write_constants(&mut self) -> Result<()> {
        if self.constants.is_empty() {
            return Ok(());
        }
        self.stream.enter_subblock(CONSTANTS_BLOCK, 4);
        let mut current_type = None;
        let constants = self.constants.clone();
        for id in constants {
            let constant = self.module.constant(id).clone();
            let type_id = self.type_index(constant.ty)?;
            if current_type != Some(type_id) {
                self.stream.emit_record(CST_SETTYPE, &[type_id as u64]);
                current_type = Some(type_id);
            }
            match constant.kind {
                ConstKind::Null | ConstKind::ZeroInitializer => {
                    self.stream.emit_record(CST_NULL, &[]);
                }
                ConstKind::Undef => self.stream.emit_record(CST_UNDEF, &[]),
                ConstKind::Poison => self.stream.emit_record(CST_POISON, &[]),
                ConstKind::Int(value) => {
                    let bits = self.module.types.get(constant.ty).int_bits().unwrap_or(64);
                    let signed = sign_extend(value, bits);
                    self.stream
                        .emit_record(CST_INTEGER, &[encode_signed(signed)]);
                }
                ConstKind::Float(bits) => {
                    let words = self.float_words(constant.ty, bits)?;
                    self.stream.emit_record(CST_FLOAT, &words);
                }
                ConstKind::String(bytes) => {
                    let operands: Vec<u64> = bytes.iter().map(|b| *b as u64).collect();
                    self.stream.emit_record(CST_STRING, &operands);
                }
                ConstKind::Aggregate(elements) => {
                    let mut operands = Vec::with_capacity(elements.len());
                    for element in elements {
                        operands.push(self.const_value_id(element)? as u64);
                    }
                    self.stream.emit_record(CST_AGGREGATE, &operands);
                }
                ConstKind::Global(_) | ConstKind::Function(_) => {
                    return Err(Error::bitcode("global constant emitted as a record"));
                }
            }
        }
        self.stream.exit_block();
        Ok(())
    }

    fn float_words(&self, ty: TypeId, bits: u64) -> Result<Vec<u64>> {
        match self.module.types.get(ty) {
            Type::Float(llvm_rust_ir::FloatKind::Half)
            | Type::Float(llvm_rust_ir::FloatKind::BFloat) => Ok(vec![bits & 0xffff]),
            Type::Float(llvm_rust_ir::FloatKind::Float) => Ok(vec![bits & 0xffff_ffff]),
            Type::Float(llvm_rust_ir::FloatKind::Double) => Ok(vec![bits]),
            Type::Float(llvm_rust_ir::FloatKind::Fp128)
            | Type::Float(llvm_rust_ir::FloatKind::PpcFp128) => {
                Ok(vec![bits & 0xffff_ffff_ffff_ffff, 0])
            }
            Type::Float(llvm_rust_ir::FloatKind::X86Fp80) => Ok(vec![bits]),
            _ => Err(Error::bitcode("float constant with a non-float type")),
        }
    }

    fn write_bodies(&mut self) -> Result<()> {
        let functions: Vec<(FuncId, Function)> = self
            .module
            .functions
            .iter()
            .map(|(id, function)| (id, function.clone()))
            .collect();
        for (_, function) in functions {
            if function.is_declaration {
                continue;
            }
            self.write_body(&function)?;
        }
        Ok(())
    }

    fn write_body(&mut self, function: &Function) -> Result<()> {
        self.stream.enter_subblock(FUNCTION_BLOCK, 4);
        self.stream
            .emit_record(FUNC_DECLAREBLOCKS, &[function.blocks.len() as u64]);
        let mut next_local = function.params.len() as u32;
        for (_, block) in function.blocks_in_order() {
            for inst_id in &block.insts {
                let inst = function.insts.get(*inst_id);
                let inst_value = self.base_local + next_local;
                self.write_instruction(function, inst, inst_value)?;
                if inst.result.is_some() {
                    next_local += 1;
                }
            }
        }
        self.write_function_vst(function);
        self.stream.exit_block();
        Ok(())
    }

    fn write_function_vst(&mut self, function: &Function) {
        let has_names = function.locals.iter().any(|local| local.name.is_some())
            || function
                .blocks_in_order()
                .any(|(_, block)| block.name.is_some());
        if !has_names {
            return;
        }
        self.stream.enter_subblock(VALUE_SYMTAB_BLOCK, 4);
        for (local_id, local) in function.locals.iter().enumerate() {
            if let Some(name) = local.name {
                let mut operands = vec![(self.base_local + local_id as u32) as u64];
                operands.extend(self.module.name(name).bytes().map(|b| b as u64));
                self.stream.emit_record(VST_ENTRY, &operands);
            }
        }
        for (index, (_, block)) in function.blocks_in_order().enumerate() {
            if let Some(name) = block.name {
                let mut operands = vec![index as u64];
                operands.extend(self.module.name(name).bytes().map(|b| b as u64));
                self.stream.emit_record(VST_BBENTRY, &operands);
            }
        }
        self.stream.exit_block();
    }

    fn write_instruction(
        &mut self,
        function: &Function,
        inst: &llvm_rust_ir::Instruction,
        inst_id: u32,
    ) -> Result<()> {
        match &inst.kind {
            InstKind::Binary {
                op,
                flags,
                fastmath,
                lhs,
                rhs,
            } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, lhs, inst_id)?;
                self.push_value(&mut operands, rhs, inst_id)?;
                operands.push(binop_code(*op) as u64);
                let flags = binary_flags(*op, *flags, *fastmath);
                if flags != 0 {
                    operands.push(flags);
                }
                self.stream.emit_record(FUNC_INST_BINOP, &operands);
            }
            InstKind::FNeg { flags, value } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, value, inst_id)?;
                operands.push(0);
                let flags = fmf_bits(*flags);
                if flags != 0 {
                    operands.push(flags);
                }
                self.stream.emit_record(FUNC_INST_UNOP, &operands);
            }
            InstKind::ICmp { pred, lhs, rhs } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, lhs, inst_id)?;
                self.push_value(&mut operands, rhs, inst_id)?;
                operands.push(icmp_code(*pred) as u64);
                self.stream.emit_record(FUNC_INST_CMP2, &operands);
            }
            InstKind::FCmp {
                pred,
                flags,
                lhs,
                rhs,
            } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, lhs, inst_id)?;
                self.push_value(&mut operands, rhs, inst_id)?;
                operands.push(fcmp_code(*pred) as u64);
                let flags = fmf_bits(*flags);
                if flags != 0 {
                    operands.push(flags);
                }
                self.stream.emit_record(FUNC_INST_CMP2, &operands);
            }
            InstKind::Alloca {
                elem_ty,
                count,
                align,
            } => {
                let elem = self.type_index(*elem_ty)? as u64;
                let (size_ty, size_value) = match count {
                    Some(value) => (value.ty, self.operand_value_id(value)?),
                    None => {
                        let i32_ty = self
                            .module
                            .types
                            .iter()
                            .find_map(|(id, ty)| match ty {
                                Type::Int(32) => Some(id),
                                _ => None,
                            })
                            .ok_or_else(|| Error::bitcode("alloca requires an i32 type"))?;
                        let one = self
                            .i32_one
                            .ok_or_else(|| Error::bitcode("alloca requires an i32 1 constant"))?;
                        (i32_ty, self.const_value_id(one)?)
                    }
                };
                let align = encode_align(*align) as u64;
                let packed = (align & 0x1f) | (1 << 6) | ((align >> 5) << 8);
                let operands = vec![
                    elem,
                    self.type_index(size_ty)? as u64,
                    size_value as u64,
                    packed,
                ];
                self.stream.emit_record(FUNC_INST_ALLOCA, &operands);
            }
            InstKind::Load {
                ty,
                ptr,
                align,
                volatile,
            } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, ptr, inst_id)?;
                operands.push(self.type_index(*ty)? as u64);
                operands.push(encode_align(*align) as u64);
                operands.push(*volatile as u64);
                self.stream.emit_record(FUNC_INST_LOAD, &operands);
            }
            InstKind::Store {
                value,
                ptr,
                align,
                volatile,
            } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, ptr, inst_id)?;
                self.push_value_and_type(&mut operands, value, inst_id)?;
                operands.push(encode_align(*align) as u64);
                operands.push(*volatile as u64);
                self.stream.emit_record(FUNC_INST_STORE, &operands);
            }
            InstKind::Gep {
                inbounds,
                source_ty,
                ptr,
                indices,
            } => {
                let mut operands = vec![*inbounds as u64, self.type_index(*source_ty)? as u64];
                self.push_value_and_type(&mut operands, ptr, inst_id)?;
                for index in indices {
                    self.push_value_and_type(&mut operands, &index.value, inst_id)?;
                }
                self.stream.emit_record(FUNC_INST_GEP, &operands);
            }
            InstKind::Call {
                tail,
                fn_ty,
                callee,
                args,
                fastmath,
                ..
            } => {
                let mut operands = vec![0];
                let mut flags = match tail {
                    TailKind::None => 0,
                    TailKind::Tail => 1,
                    TailKind::MustTail => 1 << 14,
                    TailKind::NoTail => 1 << 16,
                };
                flags |= 1 << 15;
                let fmf = fmf_bits(*fastmath);
                if fmf != 0 {
                    flags |= 1 << 17;
                }
                operands.push(flags);
                if fmf != 0 {
                    operands.push(fmf);
                }
                operands.push(self.type_index(*fn_ty)? as u64);
                self.push_value_and_type(&mut operands, callee, inst_id)?;
                let (fixed, varargs) = match self.module.types.get(*fn_ty) {
                    Type::Function {
                        params, varargs, ..
                    } => (params.len(), *varargs),
                    _ => (args.len(), false),
                };
                for arg in args.iter().take(fixed) {
                    self.push_value(&mut operands, arg, inst_id)?;
                }
                if varargs {
                    for arg in args.iter().skip(fixed) {
                        self.push_value_and_type(&mut operands, arg, inst_id)?;
                    }
                }
                self.stream.emit_record(FUNC_INST_CALL, &operands);
            }
            InstKind::Phi { incoming } => {
                let ty = result_type(function, inst)?;
                let mut operands = vec![self.type_index(ty)? as u64];
                for (value, block) in incoming {
                    let value_id = self.operand_value_id(value)?;
                    let diff = inst_id as i64 - value_id as i64;
                    operands.push(encode_signed(diff));
                    operands.push(block_index(function, *block)? as u64);
                }
                self.stream.emit_record(FUNC_INST_PHI, &operands);
            }
            InstKind::Select {
                cond,
                on_true,
                on_false,
                fastmath,
            } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, on_true, inst_id)?;
                self.push_value(&mut operands, on_false, inst_id)?;
                self.push_value_and_type(&mut operands, cond, inst_id)?;
                let flags = fmf_bits(*fastmath);
                if flags != 0 {
                    operands.push(flags);
                }
                self.stream.emit_record(FUNC_INST_VSELECT, &operands);
            }
            InstKind::Cast { op, value, to } => {
                let mut operands = Vec::new();
                self.push_value_and_type(&mut operands, value, inst_id)?;
                operands.push(self.type_index(*to)? as u64);
                operands.push(cast_code(*op) as u64);
                self.stream.emit_record(FUNC_INST_CAST, &operands);
            }
            InstKind::Ret { value } => {
                let mut operands = Vec::new();
                if let Some(value) = value {
                    self.push_value_and_type(&mut operands, value, inst_id)?;
                }
                self.stream.emit_record(FUNC_INST_RET, &operands);
            }
            InstKind::Br { dest } => {
                self.stream
                    .emit_record(FUNC_INST_BR, &[block_index(function, *dest)? as u64]);
            }
            InstKind::CondBr {
                cond,
                then_block,
                else_block,
            } => {
                let mut operands = vec![
                    block_index(function, *then_block)? as u64,
                    block_index(function, *else_block)? as u64,
                ];
                self.push_value(&mut operands, cond, inst_id)?;
                self.stream.emit_record(FUNC_INST_BR, &operands);
            }
            InstKind::Switch {
                value,
                default,
                cases,
            } => {
                let mut operands = vec![self.type_index(value.ty)? as u64];
                self.push_value(&mut operands, value, inst_id)?;
                operands.push(block_index(function, *default)? as u64);
                for (case, block) in cases {
                    operands.push(self.const_value_id(*case)? as u64);
                    operands.push(block_index(function, *block)? as u64);
                }
                self.stream.emit_record(FUNC_INST_SWITCH, &operands);
            }
            InstKind::Unreachable => self.stream.emit_record(FUNC_INST_UNREACHABLE, &[]),
        }
        Ok(())
    }

    fn push_value(&mut self, operands: &mut Vec<u64>, value: &Operand, inst_id: u32) -> Result<()> {
        let value_id = self.operand_value_id(value)?;
        operands.push(inst_id.wrapping_sub(value_id) as u64);
        Ok(())
    }

    fn push_value_and_type(
        &mut self,
        operands: &mut Vec<u64>,
        value: &Operand,
        inst_id: u32,
    ) -> Result<()> {
        let value_id = self.operand_value_id(value)?;
        operands.push(inst_id.wrapping_sub(value_id) as u64);
        if value_id >= inst_id {
            operands.push(self.type_index(value.ty)? as u64);
        }
        Ok(())
    }

    fn operand_value_id(&self, value: &Operand) -> Result<u32> {
        match value.kind {
            OperandKind::Local(id) => Ok(self.base_local + id),
            OperandKind::Const(id) => self.const_value_id(id),
            OperandKind::Global(id) => self
                .globals
                .get(&id)
                .copied()
                .ok_or_else(|| Error::bitcode("unknown global operand")),
            OperandKind::Function(id) => self
                .functions
                .get(&id)
                .copied()
                .ok_or_else(|| Error::bitcode("unknown function operand")),
        }
    }

    fn const_value_id(&self, id: ConstId) -> Result<u32> {
        match &self.module.constant(id).kind {
            ConstKind::Global(global) => self
                .globals
                .get(global)
                .copied()
                .ok_or_else(|| Error::bitcode("unknown global constant")),
            ConstKind::Function(function) => self
                .functions
                .get(function)
                .copied()
                .ok_or_else(|| Error::bitcode("unknown function constant")),
            _ => self
                .const_ids
                .get(&id)
                .copied()
                .ok_or_else(|| Error::bitcode("unknown constant")),
        }
    }

    fn type_index(&self, ty: TypeId) -> Result<u32> {
        self.type_ids
            .get(&ty)
            .copied()
            .ok_or_else(|| Error::bitcode("type missing from bitcode type table"))
    }

    fn write_strtab(&mut self) {
        self.stream.enter_subblock(STRTAB_BLOCK, 3);
        let abbrev = self
            .stream
            .define_abbrev(&[AbbrevOp::Literal(STRTAB_BLOB as u64), AbbrevOp::Blob]);
        let strtab = self.strtab.clone();
        self.stream.emit_blob_record(abbrev, &strtab);
        self.stream.exit_block();
    }

    fn finish(self) -> Vec<u8> {
        self.stream.finish()
    }
}

fn result_type(function: &Function, inst: &llvm_rust_ir::Instruction) -> Result<TypeId> {
    inst.result
        .map(|local| function.local_type(local))
        .ok_or_else(|| Error::bitcode("instruction has no result type"))
}

fn block_index(function: &Function, block: BlockId) -> Result<u32> {
    function
        .block_order()
        .iter()
        .position(|id| *id == block)
        .map(|index| index as u32)
        .ok_or_else(|| Error::bitcode("branch to an undefined block"))
}

fn encode_align(align: Option<u64>) -> u32 {
    match align {
        Some(0) | None => 0,
        Some(value) => value.trailing_zeros() + 1,
    }
}

fn sign_extend(value: u128, bits: u32) -> i64 {
    if bits >= 64 {
        value as u64 as i64
    } else {
        let shift = 64 - bits;
        ((value as u64) << shift) as i64 >> shift
    }
}

fn encode_signed(value: i64) -> u64 {
    if value >= 0 {
        (value as u64) << 1
    } else {
        (((-(value as i128)) as u64) << 1) | 1
    }
}

fn linkage_code(linkage: Linkage) -> u32 {
    match linkage {
        Linkage::External => 0,
        Linkage::Weak => 16,
        Linkage::Appending => 2,
        Linkage::Internal => 3,
        Linkage::LinkOnce => 18,
        Linkage::ExternWeak => 7,
        Linkage::Common => 8,
        Linkage::Private => 9,
        Linkage::WeakOdr => 17,
        Linkage::LinkOnceOdr => 19,
        Linkage::AvailableExternally => 12,
    }
}

fn visibility_code(visibility: Visibility) -> u32 {
    match visibility {
        Visibility::Default => 0,
        Visibility::Hidden => 1,
        Visibility::Protected => 2,
    }
}

fn binop_code(op: BinOp) -> u32 {
    match op {
        BinOp::Add | BinOp::FAdd => 0,
        BinOp::Sub | BinOp::FSub => 1,
        BinOp::Mul | BinOp::FMul => 2,
        BinOp::UDiv => 3,
        BinOp::SDiv | BinOp::FDiv => 4,
        BinOp::URem => 5,
        BinOp::SRem | BinOp::FRem => 6,
        BinOp::Shl => 7,
        BinOp::LShr => 8,
        BinOp::AShr => 9,
        BinOp::And => 10,
        BinOp::Or => 11,
        BinOp::Xor => 12,
    }
}

fn cast_code(op: CastOp) -> u32 {
    match op {
        CastOp::Trunc => 0,
        CastOp::ZExt => 1,
        CastOp::SExt => 2,
        CastOp::FPToUI => 3,
        CastOp::FPToSI => 4,
        CastOp::UIToFP => 5,
        CastOp::SIToFP => 6,
        CastOp::FPTrunc => 7,
        CastOp::FPExt => 8,
        CastOp::PtrToInt => 9,
        CastOp::IntToPtr => 10,
        CastOp::BitCast => 11,
        CastOp::AddrSpaceCast => 12,
    }
}

fn icmp_code(pred: IcmpPred) -> u32 {
    match pred {
        IcmpPred::Eq => 32,
        IcmpPred::Ne => 33,
        IcmpPred::Ugt => 34,
        IcmpPred::Uge => 35,
        IcmpPred::Ult => 36,
        IcmpPred::Ule => 37,
        IcmpPred::Sgt => 38,
        IcmpPred::Sge => 39,
        IcmpPred::Slt => 40,
        IcmpPred::Sle => 41,
    }
}

fn fcmp_code(pred: FcmpPred) -> u32 {
    match pred {
        FcmpPred::False => 0,
        FcmpPred::Oeq => 1,
        FcmpPred::Ogt => 2,
        FcmpPred::Oge => 3,
        FcmpPred::Olt => 4,
        FcmpPred::Ole => 5,
        FcmpPred::One => 6,
        FcmpPred::Ord => 7,
        FcmpPred::Uno => 8,
        FcmpPred::Ueq => 9,
        FcmpPred::Ugt => 10,
        FcmpPred::Uge => 11,
        FcmpPred::Ult => 12,
        FcmpPred::Ule => 13,
        FcmpPred::Une => 14,
        FcmpPred::True => 15,
    }
}

fn binary_flags(op: BinOp, flags: IntFlags, fastmath: FastMath) -> u64 {
    if op.is_float() {
        return fmf_bits(fastmath);
    }
    match op {
        BinOp::UDiv | BinOp::SDiv | BinOp::URem | BinOp::SRem => flags.exact as u64,
        _ => flags.nuw as u64 | ((flags.nsw as u64) << 1),
    }
}

fn fmf_bits(flags: FastMath) -> u64 {
    if flags.fast {
        return 0xfe;
    }
    (flags.nnan as u64) << 1
        | (flags.ninf as u64) << 2
        | (flags.nsz as u64) << 3
        | (flags.arcp as u64) << 4
        | (flags.contract as u64) << 5
        | (flags.afn as u64) << 6
        | (flags.reassoc as u64) << 7
}
