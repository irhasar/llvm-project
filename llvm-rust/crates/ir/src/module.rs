//! The IR module: types, globals, functions, and constants.

use std::collections::HashMap;

use llvm_rust_support::{Arena, Idx, Interner, Symbol};

use crate::constant::{ConstId, ConstKind, Constant, ConstantPool};
use crate::function::{FuncId, Function};
use crate::linkage::{Linkage, Visibility};
use crate::ty::{FloatKind, INVALID_TYPE, Type, TypeId, TypeTable};

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
    global_names: HashMap<Symbol, GlobalId>,
    function_names: HashMap<Symbol, FuncId>,
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
    }

    /// Adds a function.
    pub fn add_function(&mut self, function: Function) -> FuncId {
        let name = function.name;
        let id = self.functions.alloc(function);
        self.function_names.insert(name, id);
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

    /// Replaces a placeholder function with its definition.
    pub fn define_function(&mut self, id: FuncId, function: Function) {
        self.functions[id] = function;
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
