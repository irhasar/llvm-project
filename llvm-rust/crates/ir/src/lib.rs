//! Arena-based LLVM IR for the Track B Rust core.
//!
//! The first vertical slice covers textual IR parse/print and a verifier for
//! a deliberately small, well-defined subset. See `parser` for the grammar.

#![forbid(unsafe_code)]

pub mod constant;
pub mod function;
pub mod inst;
pub mod linkage;
pub mod module;
pub mod names;
pub mod parser;
pub mod print;
pub mod ty;
pub mod value;
pub mod verify;

pub use constant::{ConstExpr, ConstId, ConstKind, Constant};
pub use function::{Block, BlockId, FuncId, Function, Local, LocalId, Param};
pub use inst::{
    AtomicOrdering, AtomicRmwOp, Attribute, BinOp, CastOp, FastMath, FcmpPred, GepIndex, IcmpPred,
    InstId, InstKind, Instruction, IntFlags, LandingPadClause, SyncScope, TailKind, UnOp,
};
pub use linkage::{Linkage, Visibility};
pub use module::{Global, GlobalId, GlobalKind, Module, ThreadLocal};
pub use parser::{ParseError, parse_module};
pub use print::print_module;
pub use ty::{FloatKind, Type, TypeId, TypeTable};
pub use value::{Operand, OperandKind};
pub use verify::{VerifyError, verify_module};
