//! Instructions and their operand shapes.

use core::fmt;
use std::sync::Arc;

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

/// Atomic ordering used by fences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicOrdering {
    NotAtomic,
    Unordered,
    Monotonic,
    Acquire,
    Release,
    AcquireRelease,
    SequentiallyConsistent,
}

impl AtomicOrdering {
    /// Returns the textual spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAtomic => "not_atomic",
            Self::Unordered => "unordered",
            Self::Monotonic => "monotonic",
            Self::Acquire => "acquire",
            Self::Release => "release",
            Self::AcquireRelease => "acq_rel",
            Self::SequentiallyConsistent => "seq_cst",
        }
    }

    /// Parses an atomic ordering.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "not_atomic" => Self::NotAtomic,
            "unordered" => Self::Unordered,
            "monotonic" => Self::Monotonic,
            "acquire" => Self::Acquire,
            "release" => Self::Release,
            "acq_rel" => Self::AcquireRelease,
            "seq_cst" => Self::SequentiallyConsistent,
            _ => return None,
        })
    }

    /// Returns the bitcode encoding.
    pub const fn code(self) -> u64 {
        match self {
            Self::NotAtomic => 0,
            Self::Unordered => 1,
            Self::Monotonic => 2,
            Self::Acquire => 3,
            Self::Release => 4,
            Self::AcquireRelease => 5,
            Self::SequentiallyConsistent => 6,
        }
    }

    /// Decodes a bitcode ordering.
    pub const fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::NotAtomic,
            1 => Self::Unordered,
            2 => Self::Monotonic,
            3 => Self::Acquire,
            4 => Self::Release,
            5 => Self::AcquireRelease,
            6 => Self::SequentiallyConsistent,
            _ => return None,
        })
    }

    /// Returns true when the ordering is valid on a fence.
    pub const fn is_fence_valid(self) -> bool {
        matches!(
            self,
            Self::Acquire | Self::Release | Self::AcquireRelease | Self::SequentiallyConsistent
        )
    }

    /// Returns true when the ordering is valid on an atomic load.
    pub const fn is_load_valid(self) -> bool {
        matches!(
            self,
            Self::Unordered | Self::Monotonic | Self::Acquire | Self::SequentiallyConsistent
        )
    }

    /// Returns true when the ordering is valid on an atomic store.
    pub const fn is_store_valid(self) -> bool {
        matches!(
            self,
            Self::Unordered | Self::Monotonic | Self::Release | Self::SequentiallyConsistent
        )
    }

    /// Returns true when the ordering is valid on an atomicrmw.
    pub const fn is_rmw_valid(self) -> bool {
        matches!(
            self,
            Self::Monotonic
                | Self::Acquire
                | Self::Release
                | Self::AcquireRelease
                | Self::SequentiallyConsistent
        )
    }

    /// Returns true when the ordering is valid as a cmpxchg success ordering.
    pub const fn is_cmpxchg_success_valid(self) -> bool {
        self.is_rmw_valid()
    }

    /// Returns true when the ordering is valid as a cmpxchg failure ordering.
    pub const fn is_cmpxchg_failure_valid(self) -> bool {
        matches!(
            self,
            Self::Monotonic | Self::Acquire | Self::SequentiallyConsistent
        )
    }

    /// Returns true when the operation is atomic.
    pub const fn is_atomic(self) -> bool {
        !matches!(self, Self::NotAtomic)
    }
}

/// Synchronization scope used by fences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncScope {
    #[default]
    System,
    SingleThread,
}

impl SyncScope {
    /// Returns the bitcode encoding.
    pub const fn code(self) -> u64 {
        match self {
            Self::SingleThread => 0,
            Self::System => 1,
        }
    }

    /// Decodes a bitcode synchronization scope.
    pub const fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::SingleThread,
            1 => Self::System,
            _ => return None,
        })
    }
}

/// Operations supported by `atomicrmw`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicRmwOp {
    Xchg,
    Add,
    Sub,
    And,
    Nand,
    Or,
    Xor,
    Max,
    Min,
    UMax,
    UMin,
    FAdd,
    FSub,
    FMax,
    FMin,
    UIncWrap,
    UDecWrap,
}

impl AtomicRmwOp {
    /// Returns the textual spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xchg => "xchg",
            Self::Add => "add",
            Self::Sub => "sub",
            Self::And => "and",
            Self::Nand => "nand",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::Max => "max",
            Self::Min => "min",
            Self::UMax => "umax",
            Self::UMin => "umin",
            Self::FAdd => "fadd",
            Self::FSub => "fsub",
            Self::FMax => "fmax",
            Self::FMin => "fmin",
            Self::UIncWrap => "uinc_wrap",
            Self::UDecWrap => "udec_wrap",
        }
    }

    /// Returns true when the operation works on floating-point values.
    pub const fn is_float(self) -> bool {
        matches!(self, Self::FAdd | Self::FSub | Self::FMax | Self::FMin)
    }

    /// Parses an atomicrmw operation.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "xchg" => Self::Xchg,
            "add" => Self::Add,
            "sub" => Self::Sub,
            "and" => Self::And,
            "nand" => Self::Nand,
            "or" => Self::Or,
            "xor" => Self::Xor,
            "max" => Self::Max,
            "min" => Self::Min,
            "umax" => Self::UMax,
            "umin" => Self::UMin,
            "fadd" => Self::FAdd,
            "fsub" => Self::FSub,
            "fmax" => Self::FMax,
            "fmin" => Self::FMin,
            "uinc_wrap" => Self::UIncWrap,
            "udec_wrap" => Self::UDecWrap,
            _ => return None,
        })
    }

    /// Returns the bitcode encoding.
    pub const fn code(self) -> u64 {
        match self {
            Self::Xchg => 0,
            Self::Add => 1,
            Self::Sub => 2,
            Self::And => 3,
            Self::Nand => 4,
            Self::Or => 5,
            Self::Xor => 6,
            Self::Max => 7,
            Self::Min => 8,
            Self::UMax => 9,
            Self::UMin => 10,
            Self::FAdd => 11,
            Self::FSub => 12,
            Self::FMax => 13,
            Self::FMin => 14,
            Self::UIncWrap => 15,
            Self::UDecWrap => 16,
        }
    }

    /// Decodes a bitcode operation.
    pub const fn from_code(code: u64) -> Option<Self> {
        Some(match code {
            0 => Self::Xchg,
            1 => Self::Add,
            2 => Self::Sub,
            3 => Self::And,
            4 => Self::Nand,
            5 => Self::Or,
            6 => Self::Xor,
            7 => Self::Max,
            8 => Self::Min,
            9 => Self::UMax,
            10 => Self::UMin,
            11 => Self::FAdd,
            12 => Self::FSub,
            13 => Self::FMax,
            14 => Self::FMin,
            15 => Self::UIncWrap,
            16 => Self::UDecWrap,
            _ => return None,
        })
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
        ordering: AtomicOrdering,
        syncscope: SyncScope,
    },
    Store {
        value: Operand,
        ptr: Operand,
        align: Option<u64>,
        volatile: bool,
        ordering: AtomicOrdering,
        syncscope: SyncScope,
    },
    AtomicRmw {
        op: AtomicRmwOp,
        ptr: Operand,
        value: Operand,
        align: Option<u64>,
        volatile: bool,
        ordering: AtomicOrdering,
        syncscope: SyncScope,
    },
    CmpXchg {
        ptr: Operand,
        cmp: Operand,
        new: Operand,
        align: Option<u64>,
        volatile: bool,
        success: AtomicOrdering,
        failure: AtomicOrdering,
        syncscope: SyncScope,
        weak: bool,
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
        arg_attrs: Vec<Arc<Vec<Attribute>>>,
        fastmath: FastMath,
        ret_attrs: Arc<Vec<Attribute>>,
        attrs: Arc<Vec<Attribute>>,
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
    ExtractValue {
        aggregate: Operand,
        indices: Vec<u32>,
    },
    InsertValue {
        aggregate: Operand,
        value: Operand,
        indices: Vec<u32>,
    },
    ExtractElement {
        vector: Operand,
        index: Operand,
    },
    InsertElement {
        vector: Operand,
        value: Operand,
        index: Operand,
    },
    ShuffleVector {
        first: Operand,
        second: Operand,
        mask: Operand,
    },
    Freeze {
        value: Operand,
    },
    Fence {
        ordering: AtomicOrdering,
        syncscope: SyncScope,
    },
    VaArg {
        list: Operand,
        ty: TypeId,
    },
    IndirectBr {
        address: Operand,
        destinations: Vec<BlockId>,
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
                | Self::IndirectBr { .. }
                | Self::Unreachable
        )
    }
}
