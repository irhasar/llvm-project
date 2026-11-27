//! Constants and the module-level constant pool.

use llvm_rust_support::{Arena, Idx, Symbol};

use crate::function::FuncId;
use crate::inst::{CastOp, GepIndex};
use crate::module::GlobalId;
use crate::ty::TypeId;
use crate::value::Operand;

/// Handle to a constant.
pub type ConstId = Idx<Constant>;

/// A constant value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constant {
    pub ty: TypeId,
    pub kind: ConstKind,
}

/// The payload of a [`Constant`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstKind {
    /// Integer value, stored zero-extended in a `u128`.
    Int(u128),
    /// Floating-point bit pattern. `f32` uses the low 32 bits.
    Float(u64),
    Null,
    Undef,
    Poison,
    ZeroInitializer,
    /// Array, vector, or struct aggregate.
    Aggregate(Vec<ConstId>),
    /// `c"..."` byte string.
    String(Vec<u8>),
    /// Metadata string, spelled `!"..."`.
    MetadataString(String),
    /// Constant expression such as a GEP or a pointer cast.
    Expr(ConstExpr),
    Global(GlobalId),
    Function(FuncId),
    /// `blockaddress(@function, %block)`.
    BlockAddress {
        function: FuncId,
        block: Symbol,
    },
    /// `asm [sideeffect] [alignstack] [inteldialect] [unwind] "...", "..."`.
    InlineAsm {
        fn_ty: TypeId,
        asm: Vec<u8>,
        constraints: Vec<u8>,
        sideeffect: bool,
        alignstack: bool,
        inteldialect: bool,
        unwind: bool,
    },
}

/// A constant expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstExpr {
    /// `getelementptr` over constant operands.
    Gep {
        inbounds: bool,
        source_ty: TypeId,
        ptr: Operand,
        indices: Vec<GepIndex>,
    },
    /// A cast between constant operands.
    Cast { op: CastOp, value: Operand },
}

impl Constant {
    /// Creates a constant.
    pub const fn new(ty: TypeId, kind: ConstKind) -> Self {
        Self { ty, kind }
    }

    /// Returns true when the constant is an integer.
    pub const fn is_int(&self) -> bool {
        matches!(self.kind, ConstKind::Int(_))
    }
}

/// Constant pool used by one module.
pub type ConstantPool = Arena<Constant>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_kind_and_type() {
        let constant = Constant::new(3, ConstKind::Int(7));
        assert_eq!(constant.ty, 3);
        assert!(constant.is_int());
    }
}
