//! LLVM bitcode reader for the Track B IR subset.

use std::collections::HashSet;
use std::sync::Arc;

use llvm_rust_ir::ty::INVALID_TYPE;
use llvm_rust_ir::{
    Attribute, BinOp, BlockId, CastOp, ConstId, ConstKind, Constant, FastMath, FcmpPred, FuncId,
    Function, Global, GlobalId, IcmpPred, InstId, InstKind, IntFlags, Linkage, Module, Operand,
    OperandKind, TailKind, ThreadLocal, Type, TypeId, Visibility,
};
use llvm_rust_support::{Error, FastHashMap, Result};

use crate::attrs::{
    PARAMATTR_CODE_ENTRY, PARAMATTR_GRP_CODE_ENTRY, decode_attribute_group, deduplicate_attributes,
    merge_attributes,
};
use crate::bitstream::{Bitstream, Block, Item, Record};

const MODULE_BLOCK: u32 = 8;
const PARAMATTR_BLOCK: u32 = 9;
const PARAMATTR_GROUP_BLOCK: u32 = 10;
const CONSTANTS_BLOCK: u32 = 11;
const FUNCTION_BLOCK: u32 = 12;
const VALUE_SYMTAB_BLOCK: u32 = 14;
const TYPE_BLOCK: u32 = 17;
const STRTAB_BLOCK: u32 = 23;

const MODULE_VERSION: u64 = 1;
const MODULE_TRIPLE: u64 = 2;
const MODULE_DATALAYOUT: u64 = 3;
const MODULE_SECTIONNAME: u64 = 5;
const MODULE_GLOBALVAR: u64 = 7;
const MODULE_FUNCTION: u64 = 8;
const MODULE_SOURCE_FILENAME: u64 = 16;

const TYPE_VOID: u64 = 2;
const TYPE_FLOAT: u64 = 3;
const TYPE_DOUBLE: u64 = 4;
const TYPE_LABEL: u64 = 5;
const TYPE_OPAQUE: u64 = 6;
const TYPE_INTEGER: u64 = 7;
const TYPE_HALF: u64 = 10;
const TYPE_ARRAY: u64 = 11;
const TYPE_VECTOR: u64 = 12;
const TYPE_X86_FP80: u64 = 13;
const TYPE_FP128: u64 = 14;
const TYPE_PPC_FP128: u64 = 15;
const TYPE_METADATA: u64 = 16;
const TYPE_STRUCT_ANON: u64 = 18;
const TYPE_STRUCT_NAME: u64 = 19;
const TYPE_STRUCT_NAMED: u64 = 20;
const TYPE_FUNCTION: u64 = 21;
const TYPE_TOKEN: u64 = 22;
const TYPE_BFLOAT: u64 = 23;
const TYPE_OPAQUE_POINTER: u64 = 25;
const TYPE_BYTE: u64 = 27;

const CST_SETTYPE: u64 = 1;
const CST_NULL: u64 = 2;
const CST_UNDEF: u64 = 3;
const CST_INTEGER: u64 = 4;
const CST_WIDE_INTEGER: u64 = 5;
const CST_FLOAT: u64 = 6;
const CST_AGGREGATE: u64 = 7;
const CST_STRING: u64 = 8;
const CST_CSTRING: u64 = 9;
const CST_DATA: u64 = 22;
const CST_POISON: u64 = 26;

const FUNC_DECLAREBLOCKS: u64 = 1;
const FUNC_INST_BINOP: u64 = 2;
const FUNC_INST_CAST: u64 = 3;
const FUNC_INST_SELECT_OLD: u64 = 5;
const FUNC_INST_RET: u64 = 10;
const FUNC_INST_BR: u64 = 11;
const FUNC_INST_SWITCH: u64 = 12;
const FUNC_INST_UNREACHABLE: u64 = 15;
const FUNC_INST_PHI: u64 = 16;
const FUNC_INST_ALLOCA: u64 = 19;
const FUNC_INST_LOAD: u64 = 20;
const FUNC_INST_CMP2: u64 = 28;
const FUNC_INST_VSELECT: u64 = 29;
const FUNC_INST_CALL: u64 = 34;
const FUNC_INST_GEP: u64 = 43;
const FUNC_INST_STORE: u64 = 44;
const FUNC_INST_UNOP: u64 = 56;
const FUNC_INST_EXTRACTELT: u64 = 6;
const FUNC_INST_INSERTELT: u64 = 7;
const FUNC_INST_EXTRACTVAL: u64 = 26;
const FUNC_INST_INSERTVAL: u64 = 27;

const VST_ENTRY: u64 = 1;
const VST_BBENTRY: u64 = 2;
const STRTAB_BLOB: u64 = 1;

#[derive(Clone, Copy, Debug)]
enum ValueRef {
    Global(GlobalId),
    Function(FuncId),
    Constant(ConstId),
}

#[derive(Clone, Copy, Debug)]
enum LocalRef {
    Local(llvm_rust_ir::LocalId),
    Constant(ConstId),
}

struct FuncState {
    id: FuncId,
    base: u32,
    next_value: u32,
    locals: Vec<LocalRef>,
    blocks: Vec<BlockId>,
    current_block: usize,
    deferred_phis: Vec<(InstId, Record, u32, TypeId)>,
}

/// Options controlling textual memory-effect compatibility while reading.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOptions {
    /// Emit LLVM 19-compatible memory text, which has no target-memory clause.
    pub llvm19_compat: bool,
}

/// Reads a bitcode file into a module.
pub fn read_bitcode(bytes: &[u8]) -> Result<Module> {
    read_bitcode_with_options(bytes, ReadOptions::default())
}

/// Reads a bitcode file with explicit options.
pub fn read_bitcode_with_options(bytes: &[u8], options: ReadOptions) -> Result<Module> {
    let stream = Bitstream::parse(bytes)?;
    let strtab = stream
        .blocks
        .iter()
        .find(|block| block.id == STRTAB_BLOCK)
        .and_then(|block| block.records().find(|record| record.code == STRTAB_BLOB))
        .and_then(|record| record.blob.clone())
        .unwrap_or_default();
    let module_block = stream
        .blocks
        .iter()
        .find(|block| block.id == MODULE_BLOCK)
        .ok_or_else(|| Error::bitcode("bitcode has no module block"))?;
    let mut reader = Reader {
        module: Module::new(),
        strtab,
        sections: Vec::new(),
        types: Vec::new(),
        values: Vec::new(),
        pending_globals: Vec::new(),
        defined_functions: Vec::new(),
        next_body: 0,
        attr_groups: FastHashMap::default(),
        attr_lists: Vec::new(),
        attr_budget: bytes.len().saturating_mul(8).saturating_add(4096),
        next_unnamed: 0,
        llvm19_compat: options.llvm19_compat,
    };
    reader.read_module(module_block)?;
    reader.resolve_globals()?;
    reader.module.assign_unnamed_slots();
    Ok(reader.module)
}

struct Reader {
    module: Module,
    strtab: Vec<u8>,
    sections: Vec<String>,
    types: Vec<TypeId>,
    values: Vec<ValueRef>,
    pending_globals: Vec<(GlobalId, u32)>,
    defined_functions: Vec<FuncId>,
    next_body: usize,
    attr_groups: FastHashMap<u64, (u32, Arc<Vec<Attribute>>)>,
    attr_lists: Vec<Arc<Vec<(u32, Arc<Vec<Attribute>>)>>>,
    attr_budget: usize,
    next_unnamed: u32,
    llvm19_compat: bool,
}

impl Reader {
    fn read_module(&mut self, block: &Block) -> Result<()> {
        // Attribute groups can appear before the triple record. Scan it first
        // so AArch64 memory-target upgrades are decoded correctly.
        if self.module.target_triple.is_none() {
            for item in &block.items {
                if let Item::Record(record) = item {
                    if record.code == MODULE_TRIPLE {
                        self.module.target_triple =
                            Some(bytes_to_utf8(&record.operands).ok_or_else(|| {
                                Error::bitcode("target triple is not valid UTF-8")
                            })?);
                        break;
                    }
                }
            }
        }
        for item in &block.items {
            match item {
                Item::Record(record) => match record.code {
                    MODULE_VERSION => {}
                    MODULE_TRIPLE => {
                        self.module.target_triple =
                            Some(bytes_to_utf8(&record.operands).ok_or_else(|| {
                                Error::bitcode("target triple is not valid UTF-8")
                            })?);
                    }
                    MODULE_DATALAYOUT => {
                        self.module.data_layout = Some(
                            bytes_to_utf8(&record.operands)
                                .ok_or_else(|| Error::bitcode("data layout is not valid UTF-8"))?,
                        );
                    }
                    MODULE_SOURCE_FILENAME => {
                        self.module.source_filename =
                            Some(bytes_to_utf8(&record.operands).ok_or_else(|| {
                                Error::bitcode("source filename is not valid UTF-8")
                            })?);
                    }
                    MODULE_SECTIONNAME => {
                        let name = bytes_to_utf8(&record.operands)
                            .ok_or_else(|| Error::bitcode("section name is not valid UTF-8"))?;
                        self.sections.push(name);
                    }
                    MODULE_GLOBALVAR => self.read_global(record)?,
                    MODULE_FUNCTION => self.read_function(record)?,
                    _ => {}
                },
                Item::Block(child) => match child.id {
                    PARAMATTR_GROUP_BLOCK => self.read_attr_groups(child)?,
                    PARAMATTR_BLOCK => self.read_attr_lists(child)?,
                    TYPE_BLOCK => self.read_types(child)?,
                    CONSTANTS_BLOCK => {
                        self.read_constants(child, true)?;
                    }
                    FUNCTION_BLOCK => self.read_body(child)?,
                    _ => {}
                },
            }
        }
        Ok(())
    }

    fn read_attr_groups(&mut self, block: &Block) -> Result<()> {
        for record in block.records() {
            if record.code != PARAMATTR_GRP_CODE_ENTRY {
                continue;
            }
            let (group, index, mut attrs) = decode_attribute_group(
                record,
                &self.module.types,
                &self.types,
                self.module.target_triple.as_deref(),
                !self.llvm19_compat,
            )?;
            deduplicate_attributes(&mut attrs);
            self.attr_budget = self
                .attr_budget
                .checked_sub(attrs.len())
                .ok_or_else(|| Error::bitcode("attribute groups exceed the input size budget"))?;
            self.attr_groups.insert(group, (index, Arc::new(attrs)));
        }
        Ok(())
    }

    fn read_attr_lists(&mut self, block: &Block) -> Result<()> {
        for record in block.records() {
            if record.code != PARAMATTR_CODE_ENTRY {
                continue;
            }
            let mut list: Vec<(u32, Arc<Vec<Attribute>>)> = Vec::new();
            let mut positions: FastHashMap<u32, usize> = FastHashMap::default();
            let mut merged_slots: HashSet<u32> = HashSet::new();
            for group in &record.operands {
                let (index, attrs) =
                    self.attr_groups.get(group).cloned().ok_or_else(|| {
                        Error::bitcode(format!("unknown attribute group {group}"))
                    })?;
                let Some(&position) = positions.get(&index) else {
                    positions.insert(index, list.len());
                    list.push((index, attrs));
                    continue;
                };
                if attrs.is_empty() || Arc::ptr_eq(&list[position].1, &attrs) {
                    continue;
                }
                let target_len = list[position].1.len();
                let mut merged = Vec::with_capacity(target_len + attrs.len());
                merged.extend(list[position].1.iter().cloned());
                merge_attributes(&mut merged, &attrs);
                let charge = if merged_slots.insert(index) {
                    merged.len()
                } else {
                    merged.len().saturating_sub(target_len)
                };
                self.attr_budget = self.attr_budget.checked_sub(charge).ok_or_else(|| {
                    Error::bitcode("merged attribute lists exceed the input size budget")
                })?;
                list[position].1 = Arc::new(merged);
            }
            self.attr_lists.push(Arc::new(list));
        }
        Ok(())
    }

    fn attribute_list(&self, id: u64) -> Result<Arc<Vec<(u32, Arc<Vec<Attribute>>)>>> {
        if id == 0 {
            return Ok(Arc::new(Vec::new()));
        }
        let index =
            usize::try_from(id - 1).map_err(|_| Error::bitcode("attribute list index overflow"))?;
        self.attr_lists
            .get(index)
            .cloned()
            .ok_or_else(|| Error::bitcode(format!("unknown attribute list {id}")))
    }

    fn read_types(&mut self, block: &Block) -> Result<()> {
        let mut pending_name: Option<String> = None;
        for record in block.records() {
            match record.code {
                1 => {}
                TYPE_STRUCT_NAME => {
                    pending_name = Some(
                        bytes_to_utf8(&record.operands)
                            .ok_or_else(|| Error::bitcode("type name is not valid UTF-8"))?,
                    )
                }
                TYPE_OPAQUE if pending_name.is_some() => {
                    let name = pending_name.take().expect("checked above");
                    let id = self.module.types.define_named_opaque(&name);
                    self.types.push(id);
                    if !self.module.named_types.contains(&id) {
                        self.module.named_types.push(id);
                    }
                }
                TYPE_STRUCT_NAMED => {
                    let name = pending_name
                        .take()
                        .ok_or_else(|| Error::bitcode("named struct without a name"))?;
                    let id = self.module.types.named(&name);
                    self.types.push(id);
                    let packed = record.operands.first().copied().unwrap_or(0) != 0;
                    let mut fields = Vec::new();
                    for operand in record.operands.iter().skip(1) {
                        fields.push(self.type_at(*operand)?);
                    }
                    self.module
                        .types
                        .define_named(&name, fields, packed)
                        .map_err(Error::bitcode)?;
                    if !self.module.named_types.contains(&id) {
                        self.module.named_types.push(id);
                    }
                }
                code => {
                    let ty = self.read_type(code, record)?;
                    let id = self.module.types.intern(ty);
                    self.types.push(id);
                }
            }
        }
        Ok(())
    }

    fn read_type(&mut self, code: u64, record: &Record) -> Result<Type> {
        let ty = match code {
            TYPE_VOID => Type::Void,
            TYPE_LABEL => Type::Label,
            TYPE_TOKEN => Type::Token,
            TYPE_METADATA => Type::Metadata,
            TYPE_OPAQUE => Type::Opaque,
            TYPE_INTEGER => Type::Int(record.operands.first().copied().unwrap_or(0) as u32),
            TYPE_BYTE => Type::Int(8),
            TYPE_HALF => Type::Float(llvm_rust_ir::FloatKind::Half),
            TYPE_BFLOAT => Type::Float(llvm_rust_ir::FloatKind::BFloat),
            TYPE_FLOAT => Type::Float(llvm_rust_ir::FloatKind::Float),
            TYPE_DOUBLE => Type::Float(llvm_rust_ir::FloatKind::Double),
            TYPE_X86_FP80 => Type::Float(llvm_rust_ir::FloatKind::X86Fp80),
            TYPE_FP128 => Type::Float(llvm_rust_ir::FloatKind::Fp128),
            TYPE_PPC_FP128 => Type::Float(llvm_rust_ir::FloatKind::PpcFp128),
            TYPE_OPAQUE_POINTER => {
                let address_space = record.operands.first().copied().unwrap_or(0);
                if address_space != 0 {
                    return Err(Error::bitcode(
                        "nonzero pointer address spaces are not supported",
                    ));
                }
                Type::Pointer
            }
            TYPE_ARRAY => {
                let len = record.operands.first().copied().unwrap_or(0);
                let elem = self.type_at(*record.operands.get(1).unwrap_or(&0))?;
                Type::Array { elem, len }
            }
            TYPE_VECTOR => {
                let len = record.operands.first().copied().unwrap_or(0) as u32;
                let elem = self.type_at(*record.operands.get(1).unwrap_or(&0))?;
                let scalable = record.operands.get(2).copied().unwrap_or(0) != 0;
                Type::Vector {
                    elem,
                    len,
                    scalable,
                }
            }
            TYPE_STRUCT_ANON => {
                let packed = record.operands.first().copied().unwrap_or(0) != 0;
                let mut fields = Vec::new();
                for operand in record.operands.iter().skip(1) {
                    fields.push(self.type_at(*operand)?);
                }
                Type::Struct {
                    fields,
                    packed,
                    name: None,
                }
            }
            TYPE_FUNCTION => {
                let varargs = record.operands.first().copied().unwrap_or(0) != 0;
                let ret = self.type_at(*record.operands.get(1).unwrap_or(&0))?;
                let mut params = Vec::new();
                for operand in record.operands.iter().skip(2) {
                    params.push(self.type_at(*operand)?);
                }
                Type::Function {
                    ret,
                    params,
                    varargs,
                }
            }
            other => return Err(Error::bitcode(format!("unsupported type code {other}"))),
        };
        Ok(ty)
    }

    fn type_at(&self, index: u64) -> Result<TypeId> {
        self.types
            .get(index as usize)
            .copied()
            .ok_or_else(|| Error::bitcode(format!("unknown type index {index}")))
    }

    fn read_global(&mut self, record: &Record) -> Result<()> {
        let name = self.name_from(
            record.operands.first().copied().unwrap_or(0),
            record.operands.get(1).copied().unwrap_or(0),
        )?;
        let unnamed = name.is_empty();
        let name = if unnamed { self.unnamed_name() } else { name };
        let ty = self.type_at(record.operands.get(2).copied().unwrap_or(0))?;
        let flags = record.operands.get(3).copied().unwrap_or(0);
        if flags >> 2 != 0 {
            return Err(Error::bitcode(
                "nonzero global address spaces are not supported",
            ));
        }
        let is_const = flags & 1 != 0;
        let init = record.operands.get(4).copied().unwrap_or(0);
        let linkage = decode_linkage(record.operands.get(5).copied().unwrap_or(0));
        let align = decode_align(record.operands.get(6).copied().unwrap_or(0));
        let section = self.section_at(record.operands.get(7).copied().unwrap_or(0));
        let visibility = decode_visibility(record.operands.get(8).copied().unwrap_or(0));
        let tls_model = ThreadLocal::from_bitcode(record.operands.get(9).copied().unwrap_or(0));
        let unnamed_code = record.operands.get(10).copied().unwrap_or(0);
        let unnamed_addr = unnamed_code == 1;
        let local_unnamed_addr = unnamed_code == 2;
        let dso_local = record.operands.get(15).copied().unwrap_or(0) != 0;
        let symbol = self.module.intern(&name);
        let id = self.module.global_or_insert(symbol);
        self.module.define_global(
            id,
            Global {
                name: symbol,
                ty,
                init: None,
                linkage,
                visibility,
                is_const,
                tls_model,
                unnamed_addr,
                local_unnamed_addr,
                dso_local,
                align,
                section,
            },
        );
        if unnamed {
            self.module.mark_unnamed_global(id);
        }
        self.values.push(ValueRef::Global(id));
        if init != 0 {
            self.pending_globals.push((id, (init - 1) as u32));
        }
        Ok(())
    }

    fn read_function(&mut self, record: &Record) -> Result<()> {
        let name = self.name_from(
            record.operands.first().copied().unwrap_or(0),
            record.operands.get(1).copied().unwrap_or(0),
        )?;
        let unnamed = name.is_empty();
        let name = if unnamed { self.unnamed_name() } else { name };
        let ty = self.type_at(record.operands.get(2).copied().unwrap_or(0))?;
        let is_declaration = record.operands.get(4).copied().unwrap_or(0) != 0;
        let linkage = decode_linkage(record.operands.get(5).copied().unwrap_or(0));
        let align = decode_align(record.operands.get(7).copied().unwrap_or(0));
        let visibility = decode_visibility(record.operands.get(9).copied().unwrap_or(0));
        let unnamed_code = record.operands.get(11).copied().unwrap_or(0);
        let unnamed_addr = unnamed_code == 1;
        let local_unnamed_addr = unnamed_code == 2;
        let dso_local = record.operands.get(17).copied().unwrap_or(0) != 0;
        let symbol = self.module.intern(&name);
        let mut function = Function::new(symbol, ty);
        function.is_declaration = is_declaration;
        function.linkage = linkage;
        function.align = align;
        function.visibility = visibility;
        function.unnamed_addr = unnamed_addr;
        function.local_unnamed_addr = local_unnamed_addr;
        function.dso_local = dso_local;
        if let Type::Function { params, .. } = self.module.types.get(ty).clone() {
            for param in params {
                function.add_param(None, param, Vec::new());
            }
        }
        let attr_list = self.attribute_list(record.operands.get(6).copied().unwrap_or(0))?;
        for (index, attrs) in attr_list.iter() {
            match *index {
                0 => function.ret_attrs = attrs.clone(),
                u32::MAX => function.attrs = attrs.clone(),
                _ => {
                    let Some(param) = function.params.get_mut(*index as usize - 1) else {
                        return Err(Error::bitcode(format!(
                            "attribute index {index} is out of range"
                        )));
                    };
                    param.attrs = attrs.clone();
                }
            }
        }
        let id = self.module.function_or_insert(symbol);
        self.module.define_function(id, function);
        if unnamed {
            self.module.mark_unnamed_function(id);
        }
        self.values.push(ValueRef::Function(id));
        if !is_declaration {
            self.defined_functions.push(id);
        }
        Ok(())
    }

    fn read_constants(&mut self, block: &Block, module_level: bool) -> Result<Vec<ConstId>> {
        let mut current_ty = None;
        let mut ids = Vec::new();
        for record in block.records() {
            if record.code == CST_SETTYPE {
                current_ty = Some(self.type_at(record.operands.first().copied().unwrap_or(0))?);
                continue;
            }
            let ty = current_ty.ok_or_else(|| Error::bitcode("constant without a type"))?;
            let kind = self.read_constant_kind(record.code, ty, record)?;
            let id = self.module.add_constant(Constant::new(ty, kind));
            ids.push(id);
            if module_level {
                self.values.push(ValueRef::Constant(id));
            }
        }
        Ok(ids)
    }

    /// Reads a function-local constants block.
    ///
    /// LLVM emits local constants in the function value namespace, and an
    /// aggregate may reference a later constant. Reserve the local slots
    /// before decoding so forward references resolve to stable handles.
    fn read_function_constants(&mut self, block: &Block, local_base: u32) -> Result<Vec<ConstId>> {
        let mut current_ty = None;
        let mut records = Vec::new();
        for record in block.records() {
            if record.code == CST_SETTYPE {
                current_ty = Some(self.type_at(record.operands.first().copied().unwrap_or(0))?);
                continue;
            }
            let ty = current_ty.ok_or_else(|| Error::bitcode("constant without a type"))?;
            records.push((record.code, ty, record.clone()));
        }
        let ids: Vec<ConstId> = records
            .iter()
            .map(|(_, ty, _)| {
                self.module
                    .add_constant(Constant::new(*ty, ConstKind::Null))
            })
            .collect();
        for (index, (code, ty, record)) in records.iter().enumerate() {
            let kind =
                self.read_constant_kind_with_locals(*code, *ty, record, Some(&ids), local_base)?;
            self.module.constants[ids[index]] = Constant::new(*ty, kind);
        }
        Ok(ids)
    }

    fn read_constant_kind(&mut self, code: u64, ty: TypeId, record: &Record) -> Result<ConstKind> {
        self.read_constant_kind_with_locals(code, ty, record, None, 0)
    }

    fn read_constant_kind_with_locals(
        &mut self,
        code: u64,
        ty: TypeId,
        record: &Record,
        locals: Option<&[ConstId]>,
        local_base: u32,
    ) -> Result<ConstKind> {
        Ok(match code {
            CST_NULL => match self.module.types.get(ty) {
                Type::Int(_) => ConstKind::Int(0),
                Type::Float(_) => ConstKind::Float(0),
                Type::Array { .. } | Type::Struct { .. } | Type::Vector { .. } => {
                    ConstKind::ZeroInitializer
                }
                _ => ConstKind::Null,
            },
            CST_UNDEF => ConstKind::Undef,
            CST_POISON => ConstKind::Poison,
            CST_INTEGER => {
                let value = decode_signed(record.operands.first().copied().unwrap_or(0));
                ConstKind::Int(value as u128)
            }
            CST_WIDE_INTEGER => {
                if record.operands.is_empty() {
                    return Err(Error::bitcode("empty wide integer constant"));
                }
                let mut value = 0u128;
                for (index, operand) in record.operands.iter().enumerate() {
                    let word = decode_signed(*operand) as u64;
                    if index >= 2 {
                        if word != 0 {
                            return Err(Error::bitcode(
                                "integer constants wider than 128 bits are not supported",
                            ));
                        }
                        continue;
                    }
                    value |= (word as u128) << (index * 64);
                }
                let bits = self.module.types.get(ty).int_bits().unwrap_or(128);
                if bits < 128 {
                    value &= (1u128 << bits) - 1;
                }
                ConstKind::Int(value)
            }
            CST_FLOAT => {
                let kind = match self.module.types.get(ty) {
                    Type::Float(kind) => *kind,
                    _ => return Err(Error::bitcode("float constant without a float type")),
                };
                match kind {
                    llvm_rust_ir::FloatKind::Half
                    | llvm_rust_ir::FloatKind::BFloat
                    | llvm_rust_ir::FloatKind::Float
                    | llvm_rust_ir::FloatKind::Double => {
                        ConstKind::Float(record.operands.first().copied().unwrap_or(0))
                    }
                    _ => {
                        return Err(Error::bitcode(format!(
                            "constants of type {kind:?} are not supported"
                        )));
                    }
                }
            }
            CST_STRING | CST_CSTRING => {
                let mut bytes: Vec<u8> = record.operands.iter().map(|b| *b as u8).collect();
                if code == CST_CSTRING {
                    bytes.push(0);
                }
                ConstKind::String(bytes)
            }
            CST_AGGREGATE => {
                let mut elements = Vec::new();
                for operand in &record.operands {
                    elements.push(self.resolve_const_in(*operand as u32, locals, local_base)?);
                }
                ConstKind::Aggregate(elements)
            }
            CST_DATA => {
                let mut elements = Vec::new();
                let elem_ty = match self.module.types.get(ty) {
                    Type::Array { elem, .. } => *elem,
                    Type::Vector { elem, .. } => *elem,
                    _ => return Err(Error::bitcode("data constant without an aggregate type")),
                };
                let is_float = matches!(self.module.types.get(elem_ty), Type::Float(_));
                for operand in &record.operands {
                    let kind = if is_float {
                        ConstKind::Float(*operand)
                    } else {
                        ConstKind::Int(*operand as u128)
                    };
                    let constant = Constant::new(elem_ty, kind);
                    elements.push(self.module.add_constant(constant));
                }
                ConstKind::Aggregate(elements)
            }
            other => return Err(Error::bitcode(format!("unsupported constant code {other}"))),
        })
    }

    fn resolve_globals(&mut self) -> Result<()> {
        let pending = core::mem::take(&mut self.pending_globals);
        for (global, value) in pending {
            let init = self.resolve_const(value)?;
            self.module.globals.get_mut(global).init = Some(init);
        }
        Ok(())
    }

    fn resolve_const_in(
        &mut self,
        value: u32,
        locals: Option<&[ConstId]>,
        local_base: u32,
    ) -> Result<ConstId> {
        if let Some(locals) = locals
            && value >= local_base
        {
            let index = (value - local_base) as usize;
            return locals
                .get(index)
                .copied()
                .ok_or_else(|| Error::bitcode(format!("unknown local constant value {value}")));
        }
        self.resolve_const(value)
    }

    fn resolve_const(&mut self, value: u32) -> Result<ConstId> {
        match self.values.get(value as usize).copied() {
            Some(ValueRef::Constant(id)) => Ok(id),
            Some(ValueRef::Global(id)) => {
                let ptr = self.ptr_type()?;
                Ok(self
                    .module
                    .add_constant(Constant::new(ptr, ConstKind::Global(id))))
            }
            Some(ValueRef::Function(id)) => {
                let ty = self.ptr_type()?;
                Ok(self
                    .module
                    .add_constant(Constant::new(ty, ConstKind::Function(id))))
            }
            None => Err(Error::bitcode(format!("unknown constant value {value}"))),
        }
    }

    fn unnamed_name(&mut self) -> String {
        let slot = self.next_unnamed;
        self.next_unnamed += 1;
        format!("\0unnamed{slot}")
    }

    fn name_from(&self, offset: u64, size: u64) -> Result<String> {
        let start =
            usize::try_from(offset).map_err(|_| Error::bitcode("string table offset overflow"))?;
        let size =
            usize::try_from(size).map_err(|_| Error::bitcode("string table size overflow"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| Error::bitcode("string table range overflow"))?;
        if end > self.strtab.len() {
            return Ok(String::new());
        }
        String::from_utf8(self.strtab[start..end].to_vec())
            .map_err(|_| Error::bitcode("bitcode name is not valid UTF-8"))
    }

    fn section_at(&self, index: u64) -> Option<String> {
        if index == 0 {
            return None;
        }
        self.sections.get((index - 1) as usize).cloned()
    }

    fn read_body(&mut self, block: &Block) -> Result<()> {
        let id = *self
            .defined_functions
            .get(self.body_index())
            .ok_or_else(|| Error::bitcode("function body without a declaration"))?;
        self.next_body += 1;
        let base = self.values.len() as u32;
        let param_count = self.module.functions[id].params.len() as u32;
        let mut state = FuncState {
            id,
            base,
            next_value: base + param_count,
            locals: (0..param_count).map(LocalRef::Local).collect(),
            blocks: Vec::new(),
            current_block: 0,
            deferred_phis: Vec::new(),
        };
        let mut saw_declare_blocks = false;
        for item in &block.items {
            match item {
                Item::Record(record) => {
                    if record.code == FUNC_DECLAREBLOCKS {
                        saw_declare_blocks = true;
                        let count = record.operands.first().copied().unwrap_or(0) as usize;
                        let function = &mut self.module.functions[id];
                        function.is_declaration = false;
                        for _ in 0..count {
                            let block = function.add_block(None);
                            state.blocks.push(block);
                        }
                        function.entry = state.blocks.first().copied();
                        continue;
                    }
                    self.read_instruction(record, &mut state)?;
                }
                Item::Block(child) => match child.id {
                    CONSTANTS_BLOCK => {
                        let local_base = state.next_value;
                        let ids = self.read_function_constants(child, local_base)?;
                        state.next_value = state
                            .next_value
                            .checked_add(ids.len() as u32)
                            .ok_or_else(|| Error::bitcode("too many function values"))?;
                        state.locals.extend(ids.into_iter().map(LocalRef::Constant));
                    }
                    VALUE_SYMTAB_BLOCK => self.read_vst(child, &mut state)?,
                    _ => {}
                },
            }
        }
        let deferred = std::mem::take(&mut state.deferred_phis);
        for (inst, record, value_id, ty) in deferred {
            let (kind, result_ty) = self.decode_instruction(&record, value_id, &mut state)?;
            if result_ty != Some(ty) {
                return Err(Error::bitcode("phi result type mismatch"));
            }
            self.module.functions[id].insts[inst].kind = kind;
        }
        if saw_declare_blocks {
            self.assign_unnamed_slots(state.id)?;
        }
        Ok(())
    }

    fn assign_unnamed_slots(&mut self, id: FuncId) -> Result<()> {
        let mut slot = 0u32;
        let param_count = self.module.functions[id].params.len();
        for index in 0..param_count {
            if self.module.functions[id].params[index].name.is_none() {
                let symbol = self.module.intern(&slot.to_string());
                let function = &mut self.module.functions[id];
                function.params[index].name = Some(symbol);
                function.locals[index].name = Some(symbol);
                slot = slot
                    .checked_add(1)
                    .ok_or_else(|| Error::bitcode("too many unnamed values"))?;
            }
        }
        let mut order = Vec::new();
        for (block_id, block) in self.module.functions[id].blocks_in_order() {
            let mut insts = Vec::new();
            for inst_id in &block.insts {
                if let Some(local) = self.module.functions[id].insts.get(*inst_id).result {
                    insts.push(local);
                }
            }
            order.push((block_id, block.name, insts));
        }
        for (block_id, block_name, insts) in order {
            if block_name.is_none() {
                let symbol = self.module.intern(&slot.to_string());
                self.module.functions[id].blocks[block_id].name = Some(symbol);
                slot = slot
                    .checked_add(1)
                    .ok_or_else(|| Error::bitcode("too many unnamed values"))?;
            }
            for local in insts {
                if self.module.functions[id].locals[local as usize]
                    .name
                    .is_none()
                {
                    let symbol = self.module.intern(&slot.to_string());
                    self.module.functions[id].locals[local as usize].name = Some(symbol);
                    slot = slot
                        .checked_add(1)
                        .ok_or_else(|| Error::bitcode("too many unnamed values"))?;
                }
            }
        }
        Ok(())
    }

    fn body_index(&self) -> usize {
        self.next_body
    }

    fn read_vst(&mut self, block: &Block, state: &mut FuncState) -> Result<()> {
        for record in block.records() {
            match record.code {
                VST_ENTRY => {
                    let value = record.operands.first().copied().unwrap_or(0) as u32;
                    if value >= state.base {
                        let index = (value - state.base) as usize;
                        if let Some(LocalRef::Local(local)) = state.locals.get(index).copied() {
                            let name = bytes_to_utf8(&record.operands[1..])
                                .ok_or_else(|| Error::bitcode("value name is not valid UTF-8"))?;
                            let symbol = self.module.intern(&name);
                            let function = &mut self.module.functions[state.id];
                            function.locals[local as usize].name = Some(symbol);
                            if (local as usize) < function.params.len() {
                                function.params[local as usize].name = Some(symbol);
                            }
                        }
                    }
                }
                VST_BBENTRY => {
                    let index = record.operands.first().copied().unwrap_or(0) as usize;
                    if let Some(block) = state.blocks.get(index).copied() {
                        let name = bytes_to_utf8(&record.operands[1..])
                            .ok_or_else(|| Error::bitcode("block name is not valid UTF-8"))?;
                        let symbol = self.module.intern(&name);
                        self.module.functions[state.id].blocks[block].name = Some(symbol);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_value(&mut self, state: &mut FuncState, value: u32) -> Result<Operand> {
        if (value as usize) < self.values.len() {
            return Ok(match self.values[value as usize] {
                ValueRef::Global(id) => {
                    let ty = self.ptr_type()?;
                    Operand::global(ty, id)
                }
                ValueRef::Function(id) => {
                    let ty = self.ptr_type()?;
                    Operand::function(ty, id)
                }
                ValueRef::Constant(id) => {
                    let ty = self.module.constant(id).ty;
                    Operand::constant(ty, id)
                }
            });
        }
        let index = (value - state.base) as usize;
        if index >= state.locals.len() {
            let function = &mut self.module.functions[state.id];
            while state.locals.len() <= index {
                let local = function.reserve_local(None, INVALID_TYPE);
                state.locals.push(LocalRef::Local(local));
            }
        }
        let local = *state
            .locals
            .get(index)
            .ok_or_else(|| Error::bitcode(format!("unknown local value {value}")))?;
        Ok(match local {
            LocalRef::Local(id) => {
                let ty = self.module.functions[state.id].local_type(id);
                Operand::local(ty, id)
            }
            LocalRef::Constant(id) => {
                let ty = self.module.constant(id).ty;
                Operand::constant(ty, id)
            }
        })
    }

    fn decode_value(
        &mut self,
        record: &Record,
        index: usize,
        inst_id: u32,
        state: &mut FuncState,
    ) -> Result<(Operand, usize)> {
        let relative = *record
            .operands
            .get(index)
            .ok_or_else(|| Error::bitcode(format!("missing instruction operand {index}")))?;
        let value = inst_id.wrapping_sub(relative as u32);
        Ok((self.resolve_value(state, value)?, index + 1))
    }

    fn decode_value_as(
        &mut self,
        record: &Record,
        index: usize,
        inst_id: u32,
        state: &mut FuncState,
        expected: TypeId,
    ) -> Result<(Operand, usize)> {
        let (mut operand, next) = self.decode_value(record, index, inst_id, state)?;
        if operand.ty == INVALID_TYPE {
            operand.ty = expected;
            if let OperandKind::Local(id) = operand.kind {
                self.module.functions[state.id].locals[id as usize].ty = expected;
            }
        }
        Ok((operand, next))
    }

    fn decode_typed_value(
        &mut self,
        record: &Record,
        index: usize,
        inst_id: u32,
        state: &mut FuncState,
    ) -> Result<(Operand, usize)> {
        let relative = *record
            .operands
            .get(index)
            .ok_or_else(|| Error::bitcode(format!("missing instruction operand {index}")))?;
        let value = inst_id.wrapping_sub(relative as u32);
        let mut operand = self.resolve_value(state, value)?;
        let mut next = index + 1;
        if relative as u32 > inst_id {
            let type_index = *record
                .operands
                .get(next)
                .ok_or_else(|| Error::bitcode("missing instruction operand type"))?;
            operand.ty = self.type_at(type_index)?;
            if let OperandKind::Local(id) = operand.kind {
                self.module.functions[state.id].locals[id as usize].ty = operand.ty;
            }
            next += 1;
        }
        Ok((operand, next))
    }

    fn read_instruction(&mut self, record: &Record, state: &mut FuncState) -> Result<()> {
        let inst_id = state.next_value;
        let block = *state
            .blocks
            .get(state.current_block)
            .ok_or_else(|| Error::bitcode("instruction after the last block"))?;
        if record.code == FUNC_INST_PHI {
            let ty = self.type_at(
                record
                    .operands
                    .first()
                    .copied()
                    .ok_or_else(|| Error::bitcode("phi without a type"))?,
            )?;
            let index = (inst_id - state.base) as usize;
            let function = &mut self.module.functions[state.id];
            let local = match state.locals.get(index).copied() {
                Some(LocalRef::Local(local)) => local,
                Some(LocalRef::Constant(_)) => {
                    return Err(Error::bitcode("phi result slot is a constant"));
                }
                None if index == state.locals.len() => {
                    let local = function.reserve_local(None, ty);
                    state.locals.push(LocalRef::Local(local));
                    local
                }
                None => return Err(Error::bitcode("phi result slot is out of range")),
            };
            state.next_value = state
                .next_value
                .checked_add(1)
                .ok_or_else(|| Error::bitcode("too many function values"))?;
            let inst = function.add_inst(
                block,
                llvm_rust_ir::Instruction {
                    result: Some(local),
                    kind: InstKind::Unreachable,
                },
            );
            function.define_local(local, ty, inst);
            state
                .deferred_phis
                .push((inst, record.clone(), inst_id, ty));
            return Ok(());
        }
        let (kind, result_ty) = self.decode_instruction(record, inst_id, state)?;
        let function = &mut self.module.functions[state.id];
        let inst = function.add_inst(
            block,
            llvm_rust_ir::Instruction {
                result: None,
                kind: kind.clone(),
            },
        );
        if let Some(ty) = result_ty {
            let index = (inst_id - state.base) as usize;
            let local = match state.locals.get(index).copied() {
                Some(LocalRef::Local(local)) => local,
                Some(LocalRef::Constant(_)) => {
                    return Err(Error::bitcode("instruction result slot is a constant"));
                }
                None if index == state.locals.len() => {
                    let local = function.add_local(None, ty, inst);
                    state.locals.push(LocalRef::Local(local));
                    local
                }
                None => return Err(Error::bitcode("instruction result slot is out of range")),
            };
            function.define_local(local, ty, inst);
            function.insts[inst].result = Some(local);
            state.next_value = state
                .next_value
                .checked_add(1)
                .ok_or_else(|| Error::bitcode("too many function values"))?;
        }
        if kind.is_terminator() {
            state.current_block += 1;
        }
        Ok(())
    }

    fn decode_instruction(
        &mut self,
        record: &Record,
        inst_id: u32,
        state: &mut FuncState,
    ) -> Result<(InstKind, Option<TypeId>)> {
        match record.code {
            FUNC_INST_BINOP => {
                let (lhs, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (rhs, next) = self.decode_value(record, index, inst_id, state)?;
                index = next;
                let opcode = record.operands[index] as u32;
                index += 1;
                let flags = record.operands.get(index).copied().unwrap_or(0);
                let is_float = matches!(self.module.types.get(lhs.ty), Type::Float(_));
                let op = binop_from_code(opcode, is_float)?;
                let mut int_flags = IntFlags::default();
                let mut fastmath = FastMath::default();
                if is_float {
                    fastmath = decode_fastmath(flags);
                } else if matches!(op, BinOp::UDiv | BinOp::SDiv | BinOp::LShr | BinOp::AShr) {
                    int_flags.exact = flags & 1 != 0;
                } else if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl) {
                    int_flags.nuw = flags & 1 != 0;
                    int_flags.nsw = flags & 2 != 0;
                }
                Ok((
                    InstKind::Binary {
                        op,
                        flags: int_flags,
                        fastmath,
                        lhs,
                        rhs,
                    },
                    Some(lhs.ty),
                ))
            }
            FUNC_INST_UNOP => {
                let (value, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let opcode = record.operands.get(index).copied().unwrap_or(0);
                index += 1;
                let flags = record.operands.get(index).copied().unwrap_or(0);
                if opcode != 0 {
                    return Err(Error::bitcode("unsupported unary operator"));
                }
                Ok((
                    InstKind::FNeg {
                        flags: decode_fastmath(flags),
                        value,
                    },
                    Some(value.ty),
                ))
            }
            FUNC_INST_CMP2 => {
                let (lhs, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (rhs, next) = self.decode_value(record, index, inst_id, state)?;
                index = next;
                let predicate = record.operands[index] as u32;
                index += 1;
                let flags = record.operands.get(index).copied().unwrap_or(0);
                let i1 = self.i1_type()?;
                let kind = if matches!(self.module.types.get(lhs.ty), Type::Float(_)) {
                    InstKind::FCmp {
                        pred: fcmp_from_code(predicate)?,
                        flags: decode_fastmath(flags),
                        lhs,
                        rhs,
                    }
                } else {
                    InstKind::ICmp {
                        pred: icmp_from_code(predicate)?,
                        lhs,
                        rhs,
                    }
                };
                Ok((kind, Some(i1)))
            }
            FUNC_INST_ALLOCA => {
                let elem_ty = self.type_at(record.operands[0])?;
                let size_value = record.operands[2] as u32;
                let size = self.resolve_value(state, size_value)?;
                let count = match size.kind {
                    OperandKind::Const(id)
                        if self.module.constant(id).kind == ConstKind::Int(1) =>
                    {
                        None
                    }
                    _ => Some(size),
                };
                let packed = record.operands[3];
                let encoded = (packed & 0x1f) | (((packed >> 8) & 0x7) << 5);
                let align = if encoded == 0 {
                    None
                } else {
                    Some(1u64 << (encoded - 1))
                };
                let ptr = self.ptr_type()?;
                Ok((
                    InstKind::Alloca {
                        elem_ty,
                        count,
                        align,
                    },
                    Some(ptr),
                ))
            }
            FUNC_INST_LOAD => {
                let (ptr, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let ty = self.type_at(record.operands[index])?;
                index += 1;
                let align = decode_align(record.operands[index]);
                index += 1;
                let volatile = record.operands.get(index).copied().unwrap_or(0) != 0;
                Ok((
                    InstKind::Load {
                        ty,
                        ptr,
                        align,
                        volatile,
                    },
                    Some(ty),
                ))
            }
            FUNC_INST_STORE => {
                let (ptr, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (value, next) = self.decode_typed_value(record, index, inst_id, state)?;
                index = next;
                let align = decode_align(record.operands[index]);
                index += 1;
                let volatile = record.operands.get(index).copied().unwrap_or(0) != 0;
                Ok((
                    InstKind::Store {
                        value,
                        ptr,
                        align,
                        volatile,
                    },
                    None,
                ))
            }
            other => self.decode_instruction_rest(other, record, inst_id, state),
        }
    }

    fn decode_instruction_rest(
        &mut self,
        code: u64,
        record: &Record,
        inst_id: u32,
        state: &mut FuncState,
    ) -> Result<(InstKind, Option<TypeId>)> {
        match code {
            FUNC_INST_GEP => {
                let inbounds = record.operands.first().copied().unwrap_or(0) & 1 != 0;
                let source_ty = self.type_at(record.operands[1])?;
                let (ptr, mut index) = self.decode_typed_value(record, 2, inst_id, state)?;
                let mut indices = Vec::new();
                while index < record.operands.len() {
                    let (value, next) = self.decode_typed_value(record, index, inst_id, state)?;
                    indices.push(llvm_rust_ir::GepIndex {
                        ty: value.ty,
                        value,
                    });
                    index = next;
                }
                let ptr_ty = self.ptr_type()?;
                Ok((
                    InstKind::Gep {
                        inbounds,
                        source_ty,
                        ptr,
                        indices,
                    },
                    Some(ptr_ty),
                ))
            }
            FUNC_INST_CALL => {
                let mut ret_attrs = Arc::new(Vec::new());
                let mut call_attrs = Arc::new(Vec::new());
                let mut arg_attr_entries = Vec::new();
                for (index, attrs) in self
                    .attribute_list(record.operands.first().copied().unwrap_or(0))?
                    .iter()
                {
                    match *index {
                        0 => ret_attrs = attrs.clone(),
                        u32::MAX => call_attrs = attrs.clone(),
                        _ => arg_attr_entries.push((*index, attrs.clone())),
                    }
                }
                let mut index = 1;
                let flags = record.operands.get(index).copied().unwrap_or(0);
                index += 1;
                let fmf = if flags & (1 << 17) != 0 {
                    let value = record.operands.get(index).copied().unwrap_or(0);
                    index += 1;
                    decode_fastmath(value)
                } else {
                    FastMath::default()
                };
                let fn_ty = self.type_at(record.operands[index])?;
                index += 1;
                let (callee, next) = self.decode_typed_value(record, index, inst_id, state)?;
                index = next;
                let (ret_ty, fixed, varargs) = match self.module.types.get(fn_ty) {
                    Type::Function {
                        ret,
                        params,
                        varargs,
                    } => (*ret, params.len(), *varargs),
                    _ => return Err(Error::bitcode("call with a non-function type")),
                };
                let mut args = Vec::new();
                for _ in 0..fixed {
                    let (arg, next) = self.decode_value(record, index, inst_id, state)?;
                    args.push(arg);
                    index = next;
                }
                if varargs {
                    while index < record.operands.len() {
                        let (arg, next) = self.decode_typed_value(record, index, inst_id, state)?;
                        args.push(arg);
                        index = next;
                    }
                }
                let mut arg_attrs = vec![Arc::new(Vec::new()); args.len()];
                for (index, attrs) in arg_attr_entries {
                    let slot = index
                        .checked_sub(1)
                        .and_then(|value| arg_attrs.get_mut(value as usize))
                        .ok_or_else(|| {
                            Error::bitcode(format!(
                                "call argument attribute index {index} is out of range"
                            ))
                        })?;
                    *slot = attrs;
                }
                let tail = if flags & (1 << 14) != 0 {
                    TailKind::MustTail
                } else if flags & 1 != 0 {
                    TailKind::Tail
                } else if flags & (1 << 16) != 0 {
                    TailKind::NoTail
                } else {
                    TailKind::None
                };
                let result = if self.module.types.get(ret_ty).is_void() {
                    None
                } else {
                    Some(ret_ty)
                };
                Ok((
                    InstKind::Call {
                        tail,
                        ret_ty,
                        fn_ty,
                        callee,
                        args,
                        arg_attrs,
                        fastmath: fmf,
                        ret_attrs,
                        attrs: call_attrs,
                    },
                    result,
                ))
            }
            FUNC_INST_PHI => {
                let ty = self.type_at(record.operands[0])?;
                let mut incoming = Vec::new();
                let mut index = 1;
                while index + 1 < record.operands.len() {
                    let diff = decode_signed(record.operands[index]);
                    index += 1;
                    let block = self.block_at(state, record.operands[index] as usize)?;
                    index += 1;
                    let value = (inst_id as i64 - diff) as u32;
                    let value = self.resolve_value(state, value)?;
                    incoming.push((value, block));
                }
                Ok((InstKind::Phi { incoming }, Some(ty)))
            }
            FUNC_INST_VSELECT | FUNC_INST_SELECT_OLD => {
                let (on_true, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (on_false, next) = self.decode_value(record, index, inst_id, state)?;
                index = next;
                let (cond, next) = self.decode_typed_value(record, index, inst_id, state)?;
                index = next;
                let flags = record.operands.get(index).copied().unwrap_or(0);
                Ok((
                    InstKind::Select {
                        cond,
                        on_true,
                        on_false,
                        fastmath: decode_fastmath(flags),
                    },
                    Some(on_true.ty),
                ))
            }
            FUNC_INST_CAST => {
                let (value, mut index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let to = self.type_at(record.operands[index])?;
                index += 1;
                let opcode = record.operands[index] as u32;
                Ok((
                    InstKind::Cast {
                        op: cast_from_code(opcode)?,
                        value,
                        to,
                    },
                    Some(to),
                ))
            }
            FUNC_INST_RET => {
                if record.operands.is_empty() {
                    Ok((InstKind::Ret { value: None }, None))
                } else {
                    let (value, _) = self.decode_typed_value(record, 0, inst_id, state)?;
                    Ok((InstKind::Ret { value: Some(value) }, None))
                }
            }
            FUNC_INST_BR => {
                if record.operands.len() == 1 {
                    let dest = self.block_at(state, record.operands[0] as usize)?;
                    Ok((InstKind::Br { dest }, None))
                } else {
                    let then_block = self.block_at(state, record.operands[0] as usize)?;
                    let else_block = self.block_at(state, record.operands[1] as usize)?;
                    let (cond, _) = self.decode_value(record, 2, inst_id, state)?;
                    Ok((
                        InstKind::CondBr {
                            cond,
                            then_block,
                            else_block,
                        },
                        None,
                    ))
                }
            }
            FUNC_INST_SWITCH => {
                let value_ty = self.type_at(record.operands[0])?;
                let (value, mut index) = self.decode_value(record, 1, inst_id, state)?;
                let default = self.block_at(state, record.operands[index] as usize)?;
                index += 1;
                let mut cases = Vec::new();
                while index + 1 < record.operands.len() {
                    let case = self.resolve_const_state(record.operands[index] as u32, state)?;
                    index += 1;
                    let block = self.block_at(state, record.operands[index] as usize)?;
                    index += 1;
                    cases.push((case, block));
                }
                Ok((
                    InstKind::Switch {
                        value,
                        default,
                        cases,
                    },
                    None,
                ))
                .map(|(kind, ty)| {
                    let _ = value_ty;
                    (kind, ty)
                })
            }
            FUNC_INST_EXTRACTVAL => {
                let (aggregate, index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let indices = self.decode_aggregate_indices(record, index)?;
                let result = self
                    .module
                    .types
                    .aggregate_indexed(aggregate.ty, &indices)
                    .ok_or_else(|| Error::bitcode("invalid extractvalue indices"))?;
                Ok((InstKind::ExtractValue { aggregate, indices }, Some(result)))
            }
            FUNC_INST_INSERTVAL => {
                let (aggregate, index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (value, index) = self.decode_typed_value(record, index, inst_id, state)?;
                let indices = self.decode_aggregate_indices(record, index)?;
                let expected = self
                    .module
                    .types
                    .aggregate_indexed(aggregate.ty, &indices)
                    .ok_or_else(|| Error::bitcode("invalid insertvalue indices"))?;
                if expected != value.ty {
                    return Err(Error::bitcode(
                        "insertvalue element type does not match the index",
                    ));
                }
                Ok((
                    InstKind::InsertValue {
                        aggregate,
                        value,
                        indices,
                    },
                    Some(aggregate.ty),
                ))
            }
            FUNC_INST_EXTRACTELT => {
                let (vector, index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let (index, _) = self.decode_typed_value(record, index, inst_id, state)?;
                let result =
                    self.module.types.vector_element(vector.ty).ok_or_else(|| {
                        Error::bitcode("extractelement requires a vector operand")
                    })?;
                Ok((InstKind::ExtractElement { vector, index }, Some(result)))
            }
            FUNC_INST_INSERTELT => {
                let (vector, index) = self.decode_typed_value(record, 0, inst_id, state)?;
                let element_ty = self
                    .module
                    .types
                    .vector_element(vector.ty)
                    .ok_or_else(|| Error::bitcode("insertelement requires a vector operand"))?;
                let (value, index) =
                    self.decode_value_as(record, index, inst_id, state, element_ty)?;
                let (index, _) = self.decode_typed_value(record, index, inst_id, state)?;
                if value.ty != element_ty {
                    return Err(Error::bitcode(
                        "insertelement value type does not match the vector",
                    ));
                }
                Ok((
                    InstKind::InsertElement {
                        vector,
                        value,
                        index,
                    },
                    Some(vector.ty),
                ))
            }
            FUNC_INST_UNREACHABLE => Ok((InstKind::Unreachable, None)),
            other => Err(Error::bitcode(format!(
                "unsupported instruction code {other}"
            ))),
        }
    }

    fn decode_aggregate_indices(&self, record: &Record, mut index: usize) -> Result<Vec<u32>> {
        let mut indices = Vec::new();
        while index < record.operands.len() {
            indices.push(
                u32::try_from(record.operands[index])
                    .map_err(|_| Error::bitcode("aggregate index is out of range"))?,
            );
            index += 1;
        }
        if indices.is_empty() {
            return Err(Error::bitcode(
                "aggregate instruction requires at least one index",
            ));
        }
        Ok(indices)
    }

    fn resolve_const_state(&mut self, value: u32, state: &FuncState) -> Result<ConstId> {
        if (value as usize) < self.values.len() {
            return self.resolve_const(value);
        }
        let index = (value - state.base) as usize;
        match state.locals.get(index).copied() {
            Some(LocalRef::Constant(id)) => Ok(id),
            _ => Err(Error::bitcode(format!("unknown switch constant {value}"))),
        }
    }

    fn block_at(&self, state: &FuncState, index: usize) -> Result<BlockId> {
        state
            .blocks
            .get(index)
            .copied()
            .ok_or_else(|| Error::bitcode(format!("unknown basic block {index}")))
    }

    fn i1_type(&self) -> Result<TypeId> {
        self.module
            .types
            .iter()
            .find_map(|(id, ty)| match ty {
                Type::Int(1) => Some(id),
                _ => None,
            })
            .ok_or_else(|| Error::bitcode("module has no i1 type"))
    }

    fn ptr_type(&mut self) -> Result<TypeId> {
        Ok(self.module.types.pointer())
    }
}

fn binop_from_code(code: u32, is_float: bool) -> Result<BinOp> {
    Ok(match (code, is_float) {
        (0, false) => BinOp::Add,
        (1, false) => BinOp::Sub,
        (2, false) => BinOp::Mul,
        (3, false) => BinOp::UDiv,
        (4, false) => BinOp::SDiv,
        (5, false) => BinOp::URem,
        (6, false) => BinOp::SRem,
        (7, false) => BinOp::Shl,
        (8, false) => BinOp::LShr,
        (9, false) => BinOp::AShr,
        (10, false) => BinOp::And,
        (11, false) => BinOp::Or,
        (12, false) => BinOp::Xor,
        (0, true) => BinOp::FAdd,
        (1, true) => BinOp::FSub,
        (2, true) => BinOp::FMul,
        (4, true) => BinOp::FDiv,
        (6, true) => BinOp::FRem,
        _ => return Err(Error::bitcode(format!("unknown binary operator {code}"))),
    })
}

fn cast_from_code(code: u32) -> Result<CastOp> {
    Ok(match code {
        0 => CastOp::Trunc,
        1 => CastOp::ZExt,
        2 => CastOp::SExt,
        3 => CastOp::FPToUI,
        4 => CastOp::FPToSI,
        5 => CastOp::UIToFP,
        6 => CastOp::SIToFP,
        7 => CastOp::FPTrunc,
        8 => CastOp::FPExt,
        9 => CastOp::PtrToInt,
        10 => CastOp::IntToPtr,
        11 => CastOp::BitCast,
        12 => CastOp::AddrSpaceCast,
        _ => return Err(Error::bitcode(format!("unknown cast operator {code}"))),
    })
}

fn icmp_from_code(code: u32) -> Result<IcmpPred> {
    Ok(match code {
        32 => IcmpPred::Eq,
        33 => IcmpPred::Ne,
        34 => IcmpPred::Ugt,
        35 => IcmpPred::Uge,
        36 => IcmpPred::Ult,
        37 => IcmpPred::Ule,
        38 => IcmpPred::Sgt,
        39 => IcmpPred::Sge,
        40 => IcmpPred::Slt,
        41 => IcmpPred::Sle,
        _ => return Err(Error::bitcode(format!("unknown icmp predicate {code}"))),
    })
}

fn fcmp_from_code(code: u32) -> Result<FcmpPred> {
    Ok(match code {
        0 => FcmpPred::False,
        1 => FcmpPred::Oeq,
        2 => FcmpPred::Ogt,
        3 => FcmpPred::Oge,
        4 => FcmpPred::Olt,
        5 => FcmpPred::Ole,
        6 => FcmpPred::One,
        7 => FcmpPred::Ord,
        8 => FcmpPred::Uno,
        9 => FcmpPred::Ueq,
        10 => FcmpPred::Ugt,
        11 => FcmpPred::Uge,
        12 => FcmpPred::Ult,
        13 => FcmpPred::Ule,
        14 => FcmpPred::Une,
        15 => FcmpPred::True,
        _ => return Err(Error::bitcode(format!("unknown fcmp predicate {code}"))),
    })
}

fn decode_fastmath(bits: u64) -> FastMath {
    if bits & 0xfe == 0xfe {
        return FastMath {
            fast: true,
            ..FastMath::default()
        };
    }
    FastMath {
        fast: false,
        nnan: bits & (1 << 1) != 0,
        ninf: bits & (1 << 2) != 0,
        nsz: bits & (1 << 3) != 0,
        arcp: bits & (1 << 4) != 0,
        contract: bits & (1 << 5) != 0,
        afn: bits & (1 << 6) != 0,
        reassoc: bits & (1 << 7) != 0,
    }
}

fn decode_signed(value: u64) -> i64 {
    if value & 1 == 0 {
        (value >> 1) as i64
    } else if value == 1 {
        i64::MIN
    } else {
        -((value >> 1) as i64)
    }
}

fn decode_linkage(value: u64) -> Linkage {
    match value {
        0 => Linkage::External,
        2 => Linkage::Appending,
        3 => Linkage::Internal,
        7 => Linkage::ExternWeak,
        8 => Linkage::Common,
        9 => Linkage::Private,
        12 => Linkage::AvailableExternally,
        16 => Linkage::Weak,
        17 => Linkage::WeakOdr,
        18 => Linkage::LinkOnce,
        19 => Linkage::LinkOnceOdr,
        _ => Linkage::External,
    }
}

fn decode_visibility(value: u64) -> Visibility {
    match value {
        1 => Visibility::Hidden,
        2 => Visibility::Protected,
        _ => Visibility::Default,
    }
}

fn decode_align(value: u64) -> Option<u64> {
    if value == 0 {
        None
    } else {
        Some(1u64 << (value - 1))
    }
}

fn bytes_to_utf8(bytes: &[u64]) -> Option<String> {
    let raw: Vec<u8> = bytes.iter().map(|b| *b as u8).collect();
    String::from_utf8(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_reader(groups: Vec<(u64, u32, Vec<Attribute>)>) -> Reader {
        let mut attr_groups = FastHashMap::default();
        for (id, index, attrs) in groups {
            attr_groups.insert(id, (index, Arc::new(attrs)));
        }
        Reader {
            module: Module::new(),
            strtab: Vec::new(),
            sections: Vec::new(),
            types: Vec::new(),
            values: Vec::new(),
            pending_globals: Vec::new(),
            defined_functions: Vec::new(),
            next_body: 0,
            attr_groups,
            attr_lists: Vec::new(),
            attr_budget: 4096,
            next_unnamed: 0,
            llvm19_compat: false,
        }
    }

    fn attr(value: &str) -> Attribute {
        Attribute(value.to_string())
    }

    fn attr_list_record(groups: &[u64]) -> Record {
        Record {
            code: PARAMATTR_CODE_ENTRY,
            operands: groups.to_vec().into(),
            blob: None,
        }
    }

    #[test]
    fn reads_triple_before_decoding_aarch64_memory_groups() {
        let group = Block {
            id: PARAMATTR_GROUP_BLOCK,
            code_len: 0,
            items: vec![Item::Record(Record {
                code: PARAMATTR_GRP_CODE_ENTRY,
                operands: vec![1, u64::from(u32::MAX), 1, 86, (2u64 << 56) | 0x504].into(),
                blob: None,
            })],
        };
        let triple = Record {
            code: MODULE_TRIPLE,
            operands: b"aarch64-unknown-linux-gnu"
                .iter()
                .map(|byte| u64::from(*byte))
                .collect(),
            blob: None,
        };
        let block = Block {
            id: MODULE_BLOCK,
            code_len: 0,
            items: vec![Item::Block(group), Item::Record(triple)],
        };
        let mut reader = test_reader(Vec::new());
        reader.read_module(&block).expect("read module");
        assert_eq!(
            reader.attr_groups[&1].1.as_ref(),
            &[attr("memory(inaccessiblemem: read)")]
        );
    }

    #[test]
    fn merges_repeated_slots_and_shares_repeated_groups() {
        let mut reader = test_reader(vec![
            (1, 1, vec![attr("align 8"), attr("noundef")]),
            (2, 1, vec![attr("align 16")]),
            (3, u32::MAX, vec![attr("nounwind")]),
        ]);
        let block = Block {
            id: PARAMATTR_BLOCK,
            code_len: 0,
            items: vec![
                Item::Record(attr_list_record(&[1, 2, 3])),
                Item::Record(attr_list_record(&[3, 3])),
            ],
        };
        reader.read_attr_lists(&block).expect("read attr lists");

        let first = reader.attribute_list(1).expect("first list");
        assert_eq!(first.len(), 2);
        let arg = first
            .iter()
            .find(|(index, _)| *index == 1)
            .expect("arg attrs");
        assert_eq!(arg.1.as_ref(), &[attr("align 16"), attr("noundef")]);
        let function = first
            .iter()
            .find(|(index, _)| *index == u32::MAX)
            .expect("function attrs");
        assert!(Arc::ptr_eq(&function.1, &reader.attr_groups[&3].1));

        let second = reader.attribute_list(2).expect("second list");
        assert_eq!(second.len(), 1);
        assert!(Arc::ptr_eq(&second[0].1, &reader.attr_groups[&3].1));
    }

    #[test]
    fn short_element_records_return_errors() {
        let mut reader = test_reader(Vec::new());
        let mut state = FuncState {
            id: FuncId::from_raw(0),
            base: 0,
            next_value: 1,
            locals: Vec::new(),
            blocks: Vec::new(),
            current_block: 0,
            deferred_phis: Vec::new(),
        };
        for code in [
            FUNC_INST_EXTRACTVAL,
            FUNC_INST_INSERTVAL,
            FUNC_INST_EXTRACTELT,
            FUNC_INST_INSERTELT,
        ] {
            let record = Record {
                code,
                operands: Vec::new().into(),
                blob: None,
            };
            let error = reader
                .decode_instruction(&record, 1, &mut state)
                .expect_err("short record must fail");
            assert!(
                error.message.contains("missing instruction operand"),
                "{code}: {error}"
            );
        }
        let record = Record {
            code: FUNC_INST_EXTRACTVAL,
            operands: vec![0].into(),
            blob: None,
        };
        let error = reader
            .decode_aggregate_indices(&record, 1)
            .expect_err("empty aggregate path must fail");
        assert!(error.message.contains("at least one index"), "{error}");
    }
}
