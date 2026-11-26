//! Instructions and their operand shapes.

use core::fmt;

use crate::constant::ConstId;
use crate::function::{BlockId, LocalId};
use crate::ty::TypeId;
use crate::value::Operand;

/// Handle to an instruction inside a function.
pub type InstId = llvm_rust_support::Idx<Instruction>;

/// Integer binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    UDiv,
    SDiv,
    URem,
    SRem,
    Shl,
    LShr,
    AShr,
    And,
    Or,
    Xor,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,
}

impl BinOp {
    /// Returns the textual mnemonic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::UDiv => "udiv",
            Self::SDiv => "sdiv",
            Self::URem => "urem",
            Self::SRem => "srem",
            Self::Shl => "shl",
            Self::LShr => "lshr",
            Self::AShr => "ashr",
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::FAdd => "fadd",
            Self::FSub => "fsub",
            Self::FMul => "fmul",
            Self::FDiv => "fdiv",
            Self::FRem => "frem",
        }
    }

    /// Returns true for the floating-point operators.
    pub const fn is_float(self) -> bool {
        matches!(
            self,
            Self::FAdd | Self::FSub | Self::FMul | Self::FDiv | Self::FRem
        )
    }

    /// Parses a mnemonic.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "add" => Self::Add,
            "sub" => Self::Sub,
            "mul" => Self::Mul,
            "udiv" => Self::UDiv,
            "sdiv" => Self::SDiv,
            "urem" => Self::URem,
            "srem" => Self::SRem,
            "shl" => Self::Shl,
            "lshr" => Self::LShr,
            "ashr" => Self::AShr,
            "and" => Self::And,
            "or" => Self::Or,
            "xor" => Self::Xor,
            "fadd" => Self::FAdd,
            "fsub" => Self::FSub,
            "fmul" => Self::FMul,
            "fdiv" => Self::FDiv,
            "frem" => Self::FRem,
            _ => return None,
        })
    }
}

/// Unary floating-point operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    FNeg,
}

impl UnOp {
    /// Returns the textual mnemonic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FNeg => "fneg",
        }
    }
}

/// Cast operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastOp {
    Trunc,
    ZExt,
    SExt,
    FPToUI,
    FPToSI,
    UIToFP,
    SIToFP,
    FPTrunc,
    FPExt,
    PtrToInt,
    IntToPtr,
    BitCast,
    AddrSpaceCast,
}

impl CastOp {
    /// Returns the textual mnemonic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trunc => "trunc",
            Self::ZExt => "zext",
            Self::SExt => "sext",
            Self::FPToUI => "fptoui",
            Self::FPToSI => "fptosi",
            Self::UIToFP => "uitofp",
            Self::SIToFP => "sitofp",
            Self::FPTrunc => "fptrunc",
            Self::FPExt => "fpext",
            Self::PtrToInt => "ptrtoint",
            Self::IntToPtr => "inttoptr",
            Self::BitCast => "bitcast",
            Self::AddrSpaceCast => "addrspacecast",
        }
    }

    /// Parses a mnemonic.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "trunc" => Self::Trunc,
            "zext" => Self::ZExt,
            "sext" => Self::SExt,
            "fptoui" => Self::FPToUI,
            "fptosi" => Self::FPToSI,
            "uitofp" => Self::UIToFP,
            "sitofp" => Self::SIToFP,
            "fptrunc" => Self::FPTrunc,
            "fpext" => Self::FPExt,
            "ptrtoint" => Self::PtrToInt,
            "inttoptr" => Self::IntToPtr,
            "bitcast" => Self::BitCast,
            "addrspacecast" => Self::AddrSpaceCast,
            _ => return None,
        })
    }
}

/// Integer comparison predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcmpPred {
    Eq,
    Ne,
    Ugt,
    Uge,
    Ult,
    Ule,
    Sgt,
    Sge,
    Slt,
    Sle,
}

impl IcmpPred {
    /// Returns the textual predicate.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::Ugt => "ugt",
            Self::Uge => "uge",
            Self::Ult => "ult",
            Self::Ule => "ule",
            Self::Sgt => "sgt",
            Self::Sge => "sge",
            Self::Slt => "slt",
            Self::Sle => "sle",
        }
    }

    /// Parses a predicate.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "eq" => Self::Eq,
            "ne" => Self::Ne,
            "ugt" => Self::Ugt,
            "uge" => Self::Uge,
            "ult" => Self::Ult,
            "ule" => Self::Ule,
            "sgt" => Self::Sgt,
            "sge" => Self::Sge,
            "slt" => Self::Slt,
            "sle" => Self::Sle,
            _ => return None,
        })
    }
}

/// Floating-point comparison predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FcmpPred {
    False,
    Oeq,
    Ogt,
    Oge,
    Olt,
    Ole,
    One,
    Ord,
    Ueq,
    Ugt,
    Uge,
    Ult,
    Ule,
    Une,
    Uno,
    True,
}

impl FcmpPred {
    /// Returns the textual predicate.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::False => "false",
            Self::Oeq => "oeq",
            Self::Ogt => "ogt",
            Self::Oge => "oge",
            Self::Olt => "olt",
            Self::Ole => "ole",
            Self::One => "one",
            Self::Ord => "ord",
            Self::Ueq => "ueq",
            Self::Ugt => "ugt",
            Self::Uge => "uge",
            Self::Ult => "ult",
            Self::Ule => "ule",
            Self::Une => "une",
            Self::Uno => "uno",
            Self::True => "true",
        }
    }

    /// Parses a predicate.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "false" => Self::False,
            "oeq" => Self::Oeq,
            "ogt" => Self::Ogt,
            "oge" => Self::Oge,
            "olt" => Self::Olt,
            "ole" => Self::Ole,
            "one" => Self::One,
            "ord" => Self::Ord,
            "ueq" => Self::Ueq,
            "ugt" => Self::Ugt,
            "uge" => Self::Uge,
            "ult" => Self::Ult,
            "ule" => Self::Ule,
            "une" => Self::Une,
            "uno" => Self::Uno,
            "true" => Self::True,
            _ => return None,
        })
    }
}

/// Integer overflow and exactness flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntFlags {
    pub nsw: bool,
    pub nuw: bool,
    pub exact: bool,
}

impl IntFlags {
    /// Returns true when no flag is set.
    pub const fn is_empty(self) -> bool {
        !self.nsw && !self.nuw && !self.exact
    }
}

/// Fast-math flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FastMath {
    pub fast: bool,
    pub nnan: bool,
    pub ninf: bool,
    pub nsz: bool,
    pub arcp: bool,
    pub contract: bool,
    pub afn: bool,
    pub reassoc: bool,
}

impl FastMath {
    /// Returns true when no flag is set.
    pub const fn is_empty(self) -> bool {
        !self.fast
            && !self.nnan
            && !self.ninf
            && !self.nsz
            && !self.arcp
            && !self.contract
            && !self.afn
            && !self.reassoc
    }

    /// Adds one flag by name. Returns false for an unknown name.
    pub fn set(&mut self, name: &str) -> bool {
        match name {
            "fast" => self.fast = true,
            "nnan" => self.nnan = true,
            "ninf" => self.ninf = true,
            "nsz" => self.nsz = true,
            "arcp" => self.arcp = true,
            "contract" => self.contract = true,
            "afn" => self.afn = true,
            "reassoc" => self.reassoc = true,
            _ => return false,
        }
        true
    }
}

/// One `getelementptr` index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GepIndex {
    pub ty: TypeId,
    pub value: Operand,
}

/// Call tail marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TailKind {
    #[default]
    None,
    Tail,
    MustTail,
    NoTail,
}

impl TailKind {
    /// Returns the textual prefix, including a trailing space.
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Tail => "tail ",
            Self::MustTail => "musttail ",
            Self::NoTail => "notail ",
        }
    }
}

/// Raw attribute text. Track B preserves attribute spellings instead of
/// reimplementing every LLVM attribute kind in the first slice.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Attribute(pub String);

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub result: Option<LocalId>,
    pub kind: InstKind,
}

/// Instruction payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstKind {
    Binary {
        op: BinOp,
        flags: IntFlags,
        fastmath: FastMath,
        lhs: Operand,
        rhs: Operand,
    },
    FNeg {
        flags: FastMath,
        value: Operand,
    },
    ICmp {
        pred: IcmpPred,
        lhs: Operand,
        rhs: Operand,
    },
    FCmp {
        pred: FcmpPred,
        flags: FastMath,
        lhs: Operand,
        rhs: Operand,
    },
    Alloca {
        elem_ty: TypeId,
        count: Option<Operand>,
        align: Option<u64>,
    },
    Load {
        ty: TypeId,
        ptr: Operand,
        align: Option<u64>,
        volatile: bool,
    },
    Store {
        value: Operand,
        ptr: Operand,
        align: Option<u64>,
        volatile: bool,
    },
    Gep {
        inbounds: bool,
        source_ty: TypeId,
        ptr: Operand,
        indices: Vec<GepIndex>,
    },
    Call {
        tail: TailKind,
        ret_ty: TypeId,
        fn_ty: TypeId,
        callee: Operand,
        args: Vec<Operand>,
        arg_attrs: Vec<Vec<Attribute>>,
        fastmath: FastMath,
        ret_attrs: Vec<Attribute>,
        attrs: Vec<Attribute>,
    },
    Phi {
        incoming: Vec<(Operand, BlockId)>,
    },
    Select {
        cond: Operand,
        on_true: Operand,
        on_false: Operand,
        fastmath: FastMath,
    },
    Cast {
        op: CastOp,
        value: Operand,
        to: TypeId,
    },
    Ret {
        value: Option<Operand>,
    },
    Br {
        dest: BlockId,
    },
    CondBr {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },
    Switch {
        value: Operand,
        default: BlockId,
        cases: Vec<(ConstId, BlockId)>,
    },
    Unreachable,
}

impl InstKind {
    /// Returns true when the instruction ends a basic block.
    pub const fn is_terminator(&self) -> bool {
        matches!(
            self,
            Self::Ret { .. }
                | Self::Br { .. }
                | Self::CondBr { .. }
                | Self::Switch { .. }
                | Self::Unreachable
        )
    }
}
