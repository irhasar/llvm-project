//! Instruction operands.

use crate::constant::ConstId;
use crate::function::{FuncId, LocalId};
use crate::module::GlobalId;
use crate::ty::TypeId;

/// What an operand refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OperandKind {
    Local(LocalId),
    Const(ConstId),
    Global(GlobalId),
    Function(FuncId),
}

/// A typed operand. The type is kept next to the handle so the printer and
/// verifier do not need to chase every reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Operand {
    pub ty: TypeId,
    pub kind: OperandKind,
}

impl Operand {
    /// Creates a local operand.
    pub const fn local(ty: TypeId, id: LocalId) -> Self {
        Self {
            ty,
            kind: OperandKind::Local(id),
        }
    }

    /// Creates a constant operand.
    pub const fn constant(ty: TypeId, id: ConstId) -> Self {
        Self {
            ty,
            kind: OperandKind::Const(id),
        }
    }

    /// Creates a global-variable operand.
    pub const fn global(ty: TypeId, id: GlobalId) -> Self {
        Self {
            ty,
            kind: OperandKind::Global(id),
        }
    }

    /// Creates a function operand.
    pub const fn function(ty: TypeId, id: FuncId) -> Self {
        Self {
            ty,
            kind: OperandKind::Function(id),
        }
    }
}
