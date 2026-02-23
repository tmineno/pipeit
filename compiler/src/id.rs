// id.rs — Stable semantic identifiers for Pipit compiler phases (ADR-021)
//
// These IDs provide deterministic, span-independent identity for compiler
// artifacts. Allocated in source order during resolve; threaded through
// type_infer, lower, graph, and codegen alongside existing span keys
// (dual-key coexistence during migration).

/// Stable identifier for an actor call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CallId(pub u32);

/// Stable identifier for a top-level definition (const, param, define).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub u32);

/// Stable identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u32);

/// Allocator for stable IDs. Produces monotonically increasing IDs in
/// allocation (source) order, ensuring deterministic assignment.
#[derive(Debug, Default)]
pub struct IdAllocator {
    next_call: u32,
    next_def: u32,
    next_task: u32,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc_call(&mut self) -> CallId {
        let id = CallId(self.next_call);
        self.next_call += 1;
        id
    }

    pub fn alloc_def(&mut self) -> DefId {
        let id = DefId(self.next_def);
        self.next_def += 1;
        id
    }

    pub fn alloc_task(&mut self) -> TaskId {
        let id = TaskId(self.next_task);
        self.next_task += 1;
        id
    }
}
