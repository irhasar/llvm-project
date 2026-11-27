//! The IR module: types, globals, functions, and constants.

use std::collections::HashSet;

use llvm_rust_support::{Arena, FastHashMap, Idx, Interner, Symbol};

use crate::constant::{ConstId, ConstKind, Constant, ConstantPool};
use crate::function::{FuncId, Function};
use crate::inst::InstKind;
use crate::linkage::{Linkage, Visibility};
use crate::ty::{FloatKind, INVALID_TYPE, Type, TypeId, TypeTable};
use crate::value::{Operand, OperandKind};

/// Handle to a global variable.
pub type GlobalId = Idx<Global>;

/// A global variable or constant definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    pub name: Symbol,
    pub ty: TypeId,
    pub init: Option<ConstId>,
    pub linkage: Linkage,
    pub visibility: Visibility,
    pub is_const: bool,
    pub tls_model: ThreadLocal,
    pub unnamed_addr: bool,
    pub local_unnamed_addr: bool,
    pub dso_local: bool,
    pub align: Option<u64>,
    pub section: Option<String>,
}

/// Thread-local storage model for a global.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThreadLocal {
    #[default]
    None,
    GeneralDynamic,
    LocalDynamic,
    InitialExec,
    LocalExec,
}

impl ThreadLocal {
    pub const fn is_thread_local(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::GeneralDynamic => "thread_local",
            Self::LocalDynamic => "thread_local(localdynamic)",
            Self::InitialExec => "thread_local(initialexec)",
            Self::LocalExec => "thread_local(localexec)",
        }
    }

    pub const fn bitcode_code(self) -> u64 {
        match self {
            Self::None => 0,
            Self::GeneralDynamic => 1,
            Self::LocalDynamic => 2,
            Self::InitialExec => 3,
            Self::LocalExec => 4,
        }
    }

    pub const fn from_bitcode(code: u64) -> Self {
        match code {
            0 => Self::None,
            2 => Self::LocalDynamic,
            3 => Self::InitialExec,
            4 => Self::LocalExec,
            _ => Self::GeneralDynamic,
        }
    }
}

/// One LLVM module.
#[derive(Debug, Default)]
pub struct Module {
    pub source_filename: Option<String>,
    pub target_triple: Option<String>,
    pub data_layout: Option<String>,
    pub types: TypeTable,
    pub constants: ConstantPool,
    pub globals: Arena<Global>,
    pub functions: Arena<Function>,
    pub interner: Interner,
    pub named_types: Vec<TypeId>,
    pub attr_groups: Vec<(u32, String)>,
    global_names: FastHashMap<Symbol, GlobalId>,
    function_names: FastHashMap<Symbol, FuncId>,
    global_order: FastHashMap<GlobalId, u32>,
    function_order: FastHashMap<FuncId, u32>,
    next_order: u32,
    unnamed_globals: HashSet<GlobalId>,
    unnamed_functions: HashSet<FuncId>,
    unnamed_global_slots: FastHashMap<GlobalId, u32>,
    unnamed_function_slots: FastHashMap<FuncId, u32>,
}

impl Module {
    /// Creates an empty module.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns a name.
    pub fn intern(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    /// Resolves a name.
    pub fn name(&self, symbol: Symbol) -> &str {
        self.interner.resolve(symbol)
    }

    /// Adds a global variable.
    pub fn add_global(&mut self, global: Global) -> GlobalId {
        let name = global.name;
        let id = self.globals.alloc(global);
        self.global_names.insert(name, id);
        self.record_global_order(id);
        id
    }

    /// Looks up a global by name.
    pub fn global_named(&self, name: Symbol) -> Option<GlobalId> {
        self.global_names.get(&name).copied()
    }

    /// Returns the global with `name`, creating a placeholder when needed.
    pub fn global_or_insert(&mut self, name: Symbol) -> GlobalId {
        if let Some(id) = self.global_named(name) {
            return id;
        }
        let id = self.globals.alloc(Global {
            name,
            ty: INVALID_TYPE,
            init: None,
            linkage: Linkage::External,
            visibility: Visibility::Default,
            is_const: false,
            tls_model: ThreadLocal::None,
            unnamed_addr: false,
            local_unnamed_addr: false,
            dso_local: false,
            align: None,
            section: None,
        });
        self.global_names.insert(name, id);
        id
    }

    /// Replaces a placeholder global with its definition.
    pub fn define_global(&mut self, id: GlobalId, global: Global) {
        self.globals[id] = global;
        self.record_global_order(id);
    }

    /// Adds a function.
    pub fn add_function(&mut self, function: Function) -> FuncId {
        let name = function.name;
        let id = self.functions.alloc(function);
        self.function_names.insert(name, id);
        self.record_function_order(id);
        id
    }

    /// Looks up a function by name.
    pub fn function_named(&self, name: Symbol) -> Option<FuncId> {
        self.function_names.get(&name).copied()
    }

    /// Returns the function with `name`, creating a placeholder when needed.
    pub fn function_or_insert(&mut self, name: Symbol) -> FuncId {
        if let Some(id) = self.function_named(name) {
            return id;
        }
        let id = self.functions.alloc(Function::new(name, INVALID_TYPE));
        self.function_names.insert(name, id);
        id
    }

    /// Resolves constants that named a symbol before its kind was known.
    ///
    /// Forward references start as function placeholders. Once parsing is
    /// complete, constants pointing at an unresolved placeholder are retargeted
    /// to the real global or function with the same name.
    pub fn resolve_forward_references(&mut self) {
        let mut rewrites = Vec::new();
        for (id, constant) in self.constants.iter() {
            match constant.kind {
                ConstKind::Function(function)
                    if !self.types.is_valid(self.functions[function].ty) =>
                {
                    let name = self.functions[function].name;
                    if let Some(global) = self.global_named(name) {
                        rewrites.push((id, ConstKind::Global(global)));
                    }
                }
                ConstKind::Global(global) if !self.types.is_valid(self.globals[global].ty) => {
                    let name = self.globals[global].name;
                    if let Some(function) = self.function_named(name) {
                        rewrites.push((id, ConstKind::Function(function)));
                    }
                }
                _ => {}
            }
        }
        for (id, kind) in rewrites {
            self.constants[id].kind = kind;
        }
    }

    /// Retargets instruction operands that named a symbol before its kind was
    /// known, mirroring [`Self::resolve_forward_references`] for constants.
    pub fn resolve_forward_operands(&mut self) {
        let mut global_to_function = FastHashMap::default();
        let mut invalid_globals = FastHashMap::default();
        for (id, global) in self.globals.iter() {
            if !self.types.is_valid(global.ty) {
                invalid_globals.insert(id, global.name);
                if let Some(function) = self.function_named(global.name) {
                    global_to_function.insert(global.name, function);
                }
            }
        }
        let mut function_to_global = FastHashMap::default();
        let mut invalid_functions = FastHashMap::default();
        for (id, function) in self.functions.iter() {
            if !self.types.is_valid(function.ty) {
                invalid_functions.insert(id, function.name);
                if let Some(global) = self.global_named(function.name) {
                    function_to_global.insert(function.name, global);
                }
            }
        }
        if invalid_globals.is_empty() && invalid_functions.is_empty() {
            return;
        }
        let pointer = self.types.pointer();
        let functions: Vec<FuncId> = self.functions.iter().map(|(id, _)| id).collect();
        for function_id in functions {
            let insts: Vec<_> = self.functions[function_id]
                .insts
                .iter()
                .map(|(id, _)| id)
                .collect();
            for inst_id in insts {
                let inst = &mut self.functions[function_id].insts[inst_id];
                for_each_operand_mut(&mut inst.kind, |operand| match operand.kind {
                    OperandKind::Global(global) => {
                        if let Some(name) = invalid_globals.get(&global)
                            && let Some(function) = global_to_function.get(name).copied()
                        {
                            operand.kind = OperandKind::Function(function);
                            operand.ty = pointer;
                        }
                    }
                    OperandKind::Function(function) => {
                        if let Some(name) = invalid_functions.get(&function)
                            && let Some(global) = function_to_global.get(name).copied()
                        {
                            operand.kind = OperandKind::Global(global);
                            operand.ty = pointer;
                        }
                    }
                    _ => {}
                });
            }
        }
    }

    /// Replaces a placeholder function with its definition.
    pub fn define_function(&mut self, id: FuncId, function: Function) {
        self.functions[id] = function;
        self.record_function_order(id);
    }

    /// Marks a global that used an LLVM unnamed slot instead of a quoted name.
    pub fn mark_unnamed_global(&mut self, id: GlobalId) {
        self.unnamed_globals.insert(id);
        self.invalidate_unnamed_slots();
    }

    /// Marks a function that used an LLVM unnamed slot instead of a quoted name.
    pub fn mark_unnamed_function(&mut self, id: FuncId) {
        self.unnamed_functions.insert(id);
        self.invalidate_unnamed_slots();
    }

    /// Returns true when the global was written as an unnamed slot.
    pub fn is_unnamed_global(&self, id: GlobalId) -> bool {
        self.unnamed_globals.contains(&id)
    }

    /// Returns true when the function was written as an unnamed slot.
    pub fn is_unnamed_function(&self, id: FuncId) -> bool {
        self.unnamed_functions.contains(&id)
    }

    /// Returns the emitted slot number for an unnamed global.
    pub fn unnamed_global_slot(&self, id: GlobalId) -> Option<u32> {
        if let Some(slot) = self.unnamed_global_slots.get(&id) {
            return Some(*slot);
        }
        if !self.is_unnamed_global(id) {
            return None;
        }
        let mut slot = 0;
        for global in self.ordered_globals() {
            if self.is_unnamed_global(global) {
                if global == id {
                    return Some(slot);
                }
                slot += 1;
            }
        }
        None
    }

    /// Returns the emitted slot number for an unnamed function.
    pub fn unnamed_function_slot(&self, id: FuncId) -> Option<u32> {
        if let Some(slot) = self.unnamed_function_slots.get(&id) {
            return Some(*slot);
        }
        if !self.is_unnamed_function(id) {
            return None;
        }
        let mut slot = self
            .ordered_globals()
            .into_iter()
            .filter(|global| self.is_unnamed_global(*global))
            .count() as u32;
        for function in self.ordered_functions() {
            if self.is_unnamed_function(function) {
                if function == id {
                    return Some(slot);
                }
                slot += 1;
            }
        }
        None
    }

    fn invalidate_unnamed_slots(&mut self) {
        self.unnamed_global_slots.clear();
        self.unnamed_function_slots.clear();
    }

    /// Assigns unnamed slots in the order the printer and bitcode writer emit
    /// definitions: all globals first, then all functions.
    pub fn assign_unnamed_slots(&mut self) {
        self.unnamed_global_slots.clear();
        self.unnamed_function_slots.clear();
        if self.unnamed_globals.is_empty() && self.unnamed_functions.is_empty() {
            return;
        }
        let mut slot = 0;
        for id in self.ordered_globals() {
            if self.is_unnamed_global(id) {
                self.unnamed_global_slots.insert(id, slot);
                slot += 1;
            }
        }
        for id in self.ordered_functions() {
            if self.is_unnamed_function(id) {
                self.unnamed_function_slots.insert(id, slot);
                slot += 1;
            }
        }
    }

    fn record_global_order(&mut self, id: GlobalId) {
        if self.unnamed_globals.contains(&id) {
            self.invalidate_unnamed_slots();
        }
        if !self.global_order.contains_key(&id) {
            self.global_order.insert(id, self.next_order);
            self.next_order += 1;
        }
    }

    fn record_function_order(&mut self, id: FuncId) {
        if self.unnamed_functions.contains(&id) {
            self.invalidate_unnamed_slots();
        }
        if !self.function_order.contains_key(&id) {
            self.function_order.insert(id, self.next_order);
            self.next_order += 1;
        }
    }

    /// Globals in definition order, skipping unresolved placeholders.
    pub fn ordered_globals(&self) -> Vec<GlobalId> {
        let mut ids: Vec<GlobalId> = self
            .globals
            .iter()
            .filter(|(_, global)| self.types.is_valid(global.ty))
            .map(|(id, _)| id)
            .collect();
        ids.sort_by_key(|id| self.global_order.get(id).copied().unwrap_or(u32::MAX));
        ids
    }

    /// Functions in definition order, skipping unresolved placeholders.
    pub fn ordered_functions(&self) -> Vec<FuncId> {
        let mut ids: Vec<FuncId> = self
            .functions
            .iter()
            .filter(|(_, function)| self.types.is_valid(function.ty))
            .map(|(id, _)| id)
            .collect();
        ids.sort_by_key(|id| self.function_order.get(id).copied().unwrap_or(u32::MAX));
        ids
    }

    /// Adds a constant to the module pool.
    pub fn add_constant(&mut self, constant: Constant) -> ConstId {
        self.constants.alloc(constant)
    }

    /// Creates an integer constant.
    pub fn const_int(&mut self, ty: TypeId, value: u128) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Int(value)))
    }

    /// Creates a null constant.
    pub fn const_null(&mut self, ty: TypeId) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Null))
    }

    /// Creates an `undef` constant.
    pub fn const_undef(&mut self, ty: TypeId) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Undef))
    }

    /// Creates a `poison` constant.
    pub fn const_poison(&mut self, ty: TypeId) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Poison))
    }

    /// Creates a `zeroinitializer` constant.
    pub fn const_zero(&mut self, ty: TypeId) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::ZeroInitializer))
    }

    /// Creates a floating-point constant from raw bits.
    pub fn const_float_bits(&mut self, ty: TypeId, bits: u64) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Float(bits)))
    }

    /// Creates a floating-point constant from a `f64`.
    pub fn const_float(&mut self, kind: FloatKind, value: f64) -> ConstId {
        let ty = self.types.intern(Type::Float(kind));
        let bits = match kind {
            FloatKind::Half | FloatKind::BFloat => (half_bits(value) as u64) & 0xffff,
            FloatKind::Float => (value as f32).to_bits() as u64,
            FloatKind::Double => value.to_bits(),
            FloatKind::Fp128 | FloatKind::PpcFp128 => {
                // The first slice preserves the source spelling through the
                // parser; direct construction only needs the low 64 bits.
                value.to_bits()
            }
            FloatKind::X86Fp80 => value.to_bits(),
        };
        self.add_constant(Constant::new(ty, ConstKind::Float(bits)))
    }

    /// Creates an aggregate constant.
    pub fn const_aggregate(&mut self, ty: TypeId, elems: Vec<ConstId>) -> ConstId {
        self.add_constant(Constant::new(ty, ConstKind::Aggregate(elems)))
    }

    /// Returns the type of a constant.
    pub fn constant(&self, id: ConstId) -> &Constant {
        self.constants.get(id)
    }

    /// Returns the number of functions.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Returns the number of globals.
    pub fn global_count(&self) -> usize {
        self.globals.len()
    }

    /// Returns a display wrapper for a type.
    pub fn display_type(&self, id: TypeId) -> crate::ty::TypeDisplay<'_> {
        self.types.display(id)
    }

    /// Returns the function type of `id` when it has one.
    pub fn function_type(&self, id: FuncId) -> Option<(TypeId, Vec<TypeId>, bool)> {
        if !self.types.is_valid(self.functions[id].ty) {
            return None;
        }
        match self.types.get(self.functions[id].ty) {
            Type::Function {
                ret,
                params,
                varargs,
            } => Some((*ret, params.clone(), *varargs)),
            _ => None,
        }
    }
}

fn half_bits(value: f64) -> u16 {
    let bits = (value as f32).to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let unbiased = exp - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00;
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign;
        }
        let mantissa = mantissa | 0x80_0000;
        let shift = (14 - unbiased) as u32;
        return sign | (mantissa >> shift) as u16;
    }
    sign | ((unbiased as u16) << 10) | (mantissa >> 13) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_names_and_finds_symbols() {
        let mut module = Module::new();
        let name = module.intern("main");
        let i32_ty = module.types.int(32);
        let fnty = module.types.function(i32_ty, vec![], false);
        let function = Function::new(name, fnty);
        let id = module.add_function(function);
        assert_eq!(module.function_named(name), Some(id));
        assert_eq!(module.name(name), "main");
    }

    #[test]
    fn refreshes_unnamed_slots_after_module_edits() {
        let mut module = Module::new();
        let i32_ty = module.types.int(32);
        let fnty = module.types.function(i32_ty, vec![], false);
        let function_name = module.intern("\0unnamed0");
        let function = Function::new(function_name, fnty);
        let function_id = module.add_function(function);
        module.mark_unnamed_function(function_id);
        module.assign_unnamed_slots();
        assert_eq!(module.unnamed_function_slot(function_id), Some(0));

        let global_name = module.intern("\0unnamed1");
        let global = Global {
            name: global_name,
            ty: i32_ty,
            init: None,
            linkage: Linkage::External,
            visibility: Visibility::Default,
            is_const: false,
            tls_model: ThreadLocal::None,
            unnamed_addr: false,
            local_unnamed_addr: false,
            dso_local: false,
            align: None,
            section: None,
        };
        let global_id = module.add_global(global);
        module.mark_unnamed_global(global_id);
        module.assign_unnamed_slots();

        assert_eq!(module.unnamed_global_slot(global_id), Some(0));
        assert_eq!(module.unnamed_function_slot(function_id), Some(1));

        let named_name = module.intern("named");
        let named = Global {
            name: named_name,
            ty: i32_ty,
            init: None,
            linkage: Linkage::External,
            visibility: Visibility::Default,
            is_const: false,
            tls_model: ThreadLocal::None,
            unnamed_addr: false,
            local_unnamed_addr: false,
            dso_local: false,
            align: None,
            section: None,
        };
        module.add_global(named);
        assert!(!module.unnamed_function_slots.is_empty());
        assert_eq!(module.unnamed_function_slot(function_id), Some(1));
    }

    #[test]
    fn builds_constants() {
        let mut module = Module::new();
        let i32_ty = module.types.int(32);
        let id = module.const_int(i32_ty, 7);
        assert_eq!(module.constant(id).kind, ConstKind::Int(7));
        let null = module.const_null(i32_ty);
        assert_eq!(module.constant(null).kind, ConstKind::Null);
    }

    #[test]
    fn half_rounds_to_nearest() {
        assert_eq!(half_bits(1.0), 0x3c00);
        assert_eq!(half_bits(-2.0), 0xc000);
        assert_eq!(half_bits(0.0), 0);
    }
}

fn for_each_operand_mut(kind: &mut InstKind, mut visit: impl FnMut(&mut Operand)) {
    match kind {
        InstKind::Binary { lhs, rhs, .. } | InstKind::ICmp { lhs, rhs, .. } => {
            visit(lhs);
            visit(rhs);
        }
        InstKind::FCmp { lhs, rhs, .. } => {
            visit(lhs);
            visit(rhs);
        }
        InstKind::FNeg { value, .. } | InstKind::Cast { value, .. } => visit(value),
        InstKind::Alloca { count, .. } => {
            if let Some(count) = count {
                visit(count);
            }
        }
        InstKind::Load { ptr, .. } => visit(ptr),
        InstKind::Store { value, ptr, .. } => {
            visit(value);
            visit(ptr);
        }
        InstKind::Gep { ptr, .. } => visit(ptr),
        InstKind::Call { callee, args, .. } => {
            visit(callee);
            for arg in args {
                visit(arg);
            }
        }
        InstKind::Phi { incoming } => {
            for (value, _) in incoming {
                visit(value);
            }
        }
        InstKind::Select {
            cond,
            on_true,
            on_false,
            ..
        } => {
            visit(cond);
            visit(on_true);
            visit(on_false);
        }
        InstKind::Ret { value } => {
            if let Some(value) = value {
                visit(value);
            }
        }
        InstKind::CondBr { cond, .. } => visit(cond),
        InstKind::Switch { value, .. } => visit(value),
        InstKind::ExtractValue { aggregate, .. } => visit(aggregate),
        InstKind::InsertValue {
            aggregate, value, ..
        } => {
            visit(aggregate);
            visit(value);
        }
        InstKind::ExtractElement { vector, index } => {
            visit(vector);
            visit(index);
        }
        InstKind::InsertElement {
            vector,
            value,
            index,
        } => {
            visit(vector);
            visit(value);
            visit(index);
        }
        InstKind::Br { .. } | InstKind::Unreachable => {}
    }
}
