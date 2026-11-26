//! LLVM types and the interning table.
//!
//! Types are interned so equality is a `u32` comparison. Named structs are
//! interned by name, which permits forward references such as `%node` used
//! before `%node = type { ... }`.

use core::fmt;
use core::fmt::Write as _;
use std::collections::HashMap;
use std::rc::Rc;

use crate::names::write_name;

/// Handle to an interned type.
pub type TypeId = u32;

/// Sentinel for a value whose type has not been resolved yet.
pub const INVALID_TYPE: TypeId = u32::MAX;

/// Floating-point kinds that textual LLVM IR can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatKind {
    Half,
    BFloat,
    Float,
    Double,
    Fp128,
    X86Fp80,
    PpcFp128,
}

impl FloatKind {
    /// Returns the textual spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::BFloat => "bfloat",
            Self::Float => "float",
            Self::Double => "double",
            Self::Fp128 => "fp128",
            Self::X86Fp80 => "x86_fp80",
            Self::PpcFp128 => "ppc_fp128",
        }
    }

    /// Returns the storage width in bits.
    pub const fn bits(self) -> u32 {
        match self {
            Self::Half => 16,
            Self::BFloat => 16,
            Self::Float => 32,
            Self::Double => 64,
            Self::Fp128 | Self::PpcFp128 => 128,
            Self::X86Fp80 => 80,
        }
    }
}

/// One LLVM type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Void,
    Label,
    Token,
    Metadata,
    Opaque,
    Int(u32),
    Float(FloatKind),
    Pointer,
    Array {
        elem: TypeId,
        len: u64,
    },
    Vector {
        elem: TypeId,
        len: u32,
        scalable: bool,
    },
    Struct {
        fields: Vec<TypeId>,
        packed: bool,
        name: Option<Rc<str>>,
    },
    Function {
        ret: TypeId,
        params: Vec<TypeId>,
        varargs: bool,
    },
}

impl Type {
    /// Returns true for the `void` type.
    pub const fn is_void(&self) -> bool {
        matches!(self, Self::Void)
    }

    /// Returns the integer width when this is an integer type.
    pub const fn int_bits(&self) -> Option<u32> {
        match self {
            Self::Int(bits) => Some(*bits),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TypeKey {
    Void,
    Label,
    Token,
    Metadata,
    Opaque,
    Int(u32),
    Float(FloatKind),
    Pointer,
    Array(TypeId, u64),
    Vector(TypeId, u32, bool),
    Struct(Vec<TypeId>, bool),
    Named(Rc<str>),
    Function(TypeId, Vec<TypeId>, bool),
}

impl TypeKey {
    fn from_type(ty: &Type) -> Self {
        match ty {
            Type::Void => Self::Void,
            Type::Label => Self::Label,
            Type::Token => Self::Token,
            Type::Metadata => Self::Metadata,
            Type::Opaque => Self::Opaque,
            Type::Int(bits) => Self::Int(*bits),
            Type::Float(kind) => Self::Float(*kind),
            Type::Pointer => Self::Pointer,
            Type::Array { elem, len } => Self::Array(*elem, *len),
            Type::Vector {
                elem,
                len,
                scalable,
            } => Self::Vector(*elem, *len, *scalable),
            Type::Struct {
                name: Some(name), ..
            } => Self::Named(Rc::clone(name)),
            Type::Struct {
                fields,
                packed,
                name: None,
            } => Self::Struct(fields.clone(), *packed),
            Type::Function {
                ret,
                params,
                varargs,
            } => Self::Function(*ret, params.clone(), *varargs),
        }
    }
}

/// Interned type storage.
#[derive(Debug, Default)]
pub struct TypeTable {
    types: Vec<Type>,
    defined: Vec<bool>,
    opaque: Vec<bool>,
    map: HashMap<TypeKey, TypeId>,
}

impl TypeTable {
    /// Creates an empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns a type.
    ///
    /// A struct with a name is keyed by that name. Use [`Self::define_named`]
    /// to attach the field list.
    pub fn intern(&mut self, ty: Type) -> TypeId {
        let key = TypeKey::from_type(&ty);
        if let Some(&id) = self.map.get(&key) {
            return id;
        }
        let defined = !matches!(&ty, Type::Struct { name: Some(_), .. });
        let id = u32::try_from(self.types.len()).expect("too many types");
        self.types.push(ty);
        self.defined.push(defined);
        self.opaque.push(false);
        self.map.insert(key, id);
        id
    }

    /// Returns the handle for a named struct, creating a placeholder if needed.
    pub fn named(&mut self, name: &str) -> TypeId {
        let key = TypeKey::Named(Rc::from(name));
        if let Some(&id) = self.map.get(&key) {
            return id;
        }
        let stored: Rc<str> = Rc::from(name);
        let id = u32::try_from(self.types.len()).expect("too many types");
        self.types.push(Type::Struct {
            fields: Vec::new(),
            packed: false,
            name: Some(Rc::clone(&stored)),
        });
        self.defined.push(false);
        self.opaque.push(false);
        self.map.insert(TypeKey::Named(stored), id);
        id
    }

    /// Marks a named struct as an opaque definition.
    pub fn define_named_opaque(&mut self, name: &str) -> TypeId {
        let id = self.named(name);
        self.defined[id as usize] = true;
        self.opaque[id as usize] = true;
        id
    }

    /// Defines the body of a named struct.
    pub fn define_named(
        &mut self,
        name: &str,
        fields: Vec<TypeId>,
        packed: bool,
    ) -> Result<TypeId, String> {
        let id = self.named(name);
        if self.defined[id as usize] {
            return Err(format!("redefinition of type %{name}"));
        }
        self.types[id as usize] = Type::Struct {
            fields,
            packed,
            name: Some(Rc::from(name)),
        };
        self.defined[id as usize] = true;
        self.opaque[id as usize] = false;
        Ok(id)
    }

    /// Returns the type at `id`.
    pub fn get(&self, id: TypeId) -> &Type {
        &self.types[id as usize]
    }

    /// Returns true when `id` names an interned type.
    pub fn is_valid(&self, id: TypeId) -> bool {
        (id as usize) < self.types.len()
    }

    /// Returns true when a named struct has a body.
    pub fn is_defined(&self, id: TypeId) -> bool {
        self.defined[id as usize]
    }

    /// Returns true when a named type was defined as opaque.
    pub fn is_opaque(&self, id: TypeId) -> bool {
        self.opaque[id as usize]
    }

    /// Returns true when the type is a named struct.
    pub fn is_named(&self, id: TypeId) -> bool {
        matches!(self.types[id as usize], Type::Struct { name: Some(_), .. })
    }

    /// Returns the name of a named struct.
    pub fn name(&self, id: TypeId) -> Option<&str> {
        match &self.types[id as usize] {
            Type::Struct {
                name: Some(name), ..
            } => Some(name),
            _ => None,
        }
    }

    /// Returns the struct fields and packed flag.
    pub fn struct_body(&self, id: TypeId) -> Option<(&[TypeId], bool)> {
        match &self.types[id as usize] {
            Type::Struct { fields, packed, .. } => Some((fields, *packed)),
            _ => None,
        }
    }

    /// Returns the number of interned types.
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Returns true when no types are interned.
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Iterates over all types and their handles.
    pub fn iter(&self) -> impl Iterator<Item = (TypeId, &Type)> {
        self.types
            .iter()
            .enumerate()
            .map(|(id, ty)| (id as TypeId, ty))
    }

    /// Returns a display wrapper that resolves nested type handles.
    pub fn display(&self, id: TypeId) -> TypeDisplay<'_> {
        TypeDisplay { table: self, id }
    }

    /// Convenience constructors.
    pub fn void(&mut self) -> TypeId {
        self.intern(Type::Void)
    }

    /// Returns `iN`.
    pub fn int(&mut self, bits: u32) -> TypeId {
        self.intern(Type::Int(bits))
    }

    /// Returns `ptr`.
    pub fn pointer(&mut self) -> TypeId {
        self.intern(Type::Pointer)
    }

    /// Returns an array type.
    pub fn array(&mut self, elem: TypeId, len: u64) -> TypeId {
        self.intern(Type::Array { elem, len })
    }

    /// Returns a function type.
    pub fn function(&mut self, ret: TypeId, params: Vec<TypeId>, varargs: bool) -> TypeId {
        self.intern(Type::Function {
            ret,
            params,
            varargs,
        })
    }
}

/// Display helper for a [`TypeId`].
pub struct TypeDisplay<'a> {
    table: &'a TypeTable,
    id: TypeId,
}

impl fmt::Display for TypeDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.table.get(self.id) {
            Type::Void => f.write_str("void"),
            Type::Label => f.write_str("label"),
            Type::Token => f.write_str("token"),
            Type::Metadata => f.write_str("metadata"),
            Type::Opaque => f.write_str("opaque"),
            Type::Int(bits) => write!(f, "i{bits}"),
            Type::Float(kind) => f.write_str(kind.as_str()),
            Type::Pointer => f.write_str("ptr"),
            Type::Array { elem, len } => {
                write!(f, "[{len} x {}]", self.table.display(*elem))
            }
            Type::Vector {
                elem,
                len,
                scalable,
            } => {
                if *scalable {
                    write!(f, "<vscale x {len} x {}>", self.table.display(*elem))
                } else {
                    write!(f, "<{len} x {}>", self.table.display(*elem))
                }
            }
            Type::Struct {
                name: Some(name), ..
            } => {
                f.write_char('%')?;
                write_name(f, name)
            }
            Type::Struct {
                fields,
                packed,
                name: None,
            } => {
                if *packed {
                    f.write_str("<{ ")?;
                } else {
                    f.write_str("{ ")?;
                }
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", self.table.display(*field))?;
                }
                if *packed {
                    f.write_str(" }>")
                } else {
                    f.write_str(" }")
                }
            }
            Type::Function {
                ret,
                params,
                varargs,
            } => {
                write!(f, "{} (", self.table.display(*ret))?;
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", self.table.display(*param))?;
                }
                if *varargs {
                    if !params.is_empty() {
                        f.write_str(", ")?;
                    }
                    f.write_str("...")?;
                }
                f.write_char(')')
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_equal_types() {
        let mut types = TypeTable::new();
        let a = types.int(32);
        let b = types.int(32);
        let c = types.int(64);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(types.len(), 2);
        assert_eq!(types.display(a).to_string(), "i32");
    }

    #[test]
    fn named_structs_allow_forward_references() {
        let mut types = TypeTable::new();
        let node = types.named("node");
        let ptr = types.pointer();
        assert!(!types.is_defined(node));
        types.define_named("node", vec![ptr, ptr], false).unwrap();
        assert!(types.is_defined(node));
        assert_eq!(types.display(node).to_string(), "%node");
        assert!(types.define_named("node", vec![ptr], false).is_err());
    }

    #[test]
    fn prints_aggregate_and_function_types() {
        let mut types = TypeTable::new();
        let i32_ty = types.int(32);
        let i8_ty = types.int(8);
        let arr = types.array(i8_ty, 4);
        let st = types.intern(Type::Struct {
            fields: vec![i32_ty, arr],
            packed: false,
            name: None,
        });
        assert_eq!(types.display(st).to_string(), "{ i32, [4 x i8] }");
        let fnty = types.function(i32_ty, vec![i32_ty, st], true);
        assert_eq!(
            types.display(fnty).to_string(),
            "i32 (i32, { i32, [4 x i8] }, ...)"
        );
    }
}
