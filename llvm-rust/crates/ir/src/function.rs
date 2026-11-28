//! Functions, parameters, basic blocks, and local value slots.

use std::sync::Arc;

use llvm_rust_support::{Arena, FastHashMap, Idx, Symbol};

use crate::inst::{Attribute, InstId, Instruction};
use crate::linkage::{Linkage, Visibility};
use crate::ty::TypeId;
use crate::value::Operand;

/// Handle to a function in the module arena.
pub type FuncId = Idx<Function>;

/// Handle to a basic block in a function arena.
pub type BlockId = Idx<Block>;

/// Index into [`Function::locals`].
pub type LocalId = u32;

/// A parameter or instruction result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: Option<Symbol>,
    pub ty: TypeId,
    pub def: Option<InstId>,
    pub is_param: bool,
}

/// A function parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub name: Option<Symbol>,
    pub ty: TypeId,
    pub attrs: Arc<Vec<Attribute>>,
}

/// A basic block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub name: Option<Symbol>,
    pub insts: Vec<InstId>,
}

/// One LLVM function or declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: Symbol,
    pub ty: TypeId,
    pub linkage: Linkage,
    pub visibility: Visibility,
    pub dso_local: bool,
    pub unnamed_addr: bool,
    pub local_unnamed_addr: bool,
    pub align: Option<u64>,
    pub is_declaration: bool,
    pub ret_attrs: Arc<Vec<Attribute>>,
    pub attrs: Arc<Vec<Attribute>>,
    pub personality: Option<Operand>,
    pub params: Vec<Param>,
    pub locals: Vec<Local>,
    pub blocks: Arena<Block>,
    pub insts: Arena<Instruction>,
    pub entry: Option<BlockId>,
    block_order: Vec<BlockId>,
    block_names: FastHashMap<Symbol, BlockId>,
}

impl Function {
    /// Creates an empty function.
    pub fn new(name: Symbol, ty: TypeId) -> Self {
        Self {
            name,
            ty,
            linkage: Linkage::External,
            visibility: Visibility::Default,
            dso_local: false,
            unnamed_addr: false,
            local_unnamed_addr: false,
            align: None,
            is_declaration: true,
            ret_attrs: Arc::new(Vec::new()),
            attrs: Arc::new(Vec::new()),
            personality: None,
            params: Vec::new(),
            locals: Vec::new(),
            blocks: Arena::new(),
            insts: Arena::new(),
            entry: None,
            block_order: Vec::new(),
            block_names: FastHashMap::default(),
        }
    }

    /// Adds a parameter and its local slot.
    pub fn add_param(
        &mut self,
        name: Option<Symbol>,
        ty: TypeId,
        attrs: impl Into<Arc<Vec<Attribute>>>,
    ) -> LocalId {
        let id = u32::try_from(self.locals.len()).expect("too many locals");
        self.locals.push(Local {
            name,
            ty,
            def: None,
            is_param: true,
        });
        self.params.push(Param {
            name,
            ty,
            attrs: attrs.into(),
        });
        id
    }

    /// Adds an instruction result slot.
    pub fn add_local(&mut self, name: Option<Symbol>, ty: TypeId, def: InstId) -> LocalId {
        let id = u32::try_from(self.locals.len()).expect("too many locals");
        self.locals.push(Local {
            name,
            ty,
            def: Some(def),
            is_param: false,
        });
        id
    }

    /// Reserves a local slot for a value that may be referenced before its
    /// definition, as in a phi node.
    pub fn forward_local(&mut self, name: Symbol, ty: TypeId) -> LocalId {
        self.reserve_local(Some(name), ty)
    }
    /// Reserves a local slot without a name.
    pub fn reserve_local(&mut self, name: Option<Symbol>, ty: TypeId) -> LocalId {
        let id = u32::try_from(self.locals.len()).expect("too many locals");
        self.locals.push(Local {
            name,
            ty,
            def: None,
            is_param: false,
        });
        id
    }

    /// Defines a previously reserved local slot.
    pub fn define_local(&mut self, id: LocalId, ty: TypeId, def: InstId) {
        let local = &mut self.locals[id as usize];
        local.ty = ty;
        local.def = Some(def);
    }

    /// Returns a local slot.
    pub fn local(&self, id: LocalId) -> &Local {
        &self.locals[id as usize]
    }

    /// Returns the type of a local slot.
    pub fn local_type(&self, id: LocalId) -> TypeId {
        self.locals[id as usize].ty
    }

    /// Returns the name of a local slot.
    pub fn local_name(&self, id: LocalId) -> Option<Symbol> {
        self.locals[id as usize].name
    }

    /// Adds a basic block with an optional name.
    pub fn add_block(&mut self, name: Option<Symbol>) -> BlockId {
        let id = self.blocks.alloc(Block {
            name,
            insts: Vec::new(),
        });
        if let Some(name) = name {
            self.block_names.insert(name, id);
        }
        if self.entry.is_none() {
            self.entry = Some(id);
        }
        self.block_order.push(id);
        id
    }

    /// Returns the block with `name`, creating a placeholder when needed.
    ///
    /// Forward branches are resolved by this placeholder and filled in when
    /// the label is parsed.
    pub fn block_or_insert(&mut self, name: Symbol) -> BlockId {
        if let Some(&id) = self.block_names.get(&name) {
            return id;
        }
        let id = self.blocks.alloc(Block {
            name: Some(name),
            insts: Vec::new(),
        });
        self.block_names.insert(name, id);
        if self.entry.is_none() {
            self.entry = Some(id);
        }
        id
    }

    /// Records that a block was defined in the function body.
    pub fn mark_block_defined(&mut self, id: BlockId) {
        if !self.block_order.contains(&id) {
            self.block_order.push(id);
        }
    }

    /// Returns the blocks in source/definition order.
    pub fn block_order(&self) -> &[BlockId] {
        &self.block_order
    }

    /// Iterates over blocks in definition order.
    pub fn blocks_in_order(&self) -> impl Iterator<Item = (BlockId, &Block)> {
        self.block_order
            .iter()
            .map(move |id| (*id, &self.blocks[*id]))
    }

    /// Looks up an existing block by name.
    pub fn block_named(&self, name: Symbol) -> Option<BlockId> {
        self.block_names.get(&name).copied()
    }

    /// Adds an instruction to a block and returns its handle.
    pub fn add_inst(&mut self, block: BlockId, instruction: Instruction) -> InstId {
        let id = self.insts.alloc(instruction);
        self.blocks[block].insts.push(id);
        id
    }

    /// Returns the number of basic blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inst::InstKind;

    #[test]
    fn locals_and_blocks_are_stable() {
        let mut function = Function::new(Symbol::default(), 0);
        let param = function.add_param(None, 1, Vec::new());
        assert_eq!(param, 0);
        let entry = function.add_block(Some(Symbol::default()));
        let inst = function.add_inst(
            entry,
            Instruction {
                result: None,
                kind: InstKind::Unreachable,
            },
        );
        let local = function.add_local(None, 2, inst);
        assert_eq!(function.local_type(local), 2);
        assert_eq!(function.block_count(), 1);
    }
}
