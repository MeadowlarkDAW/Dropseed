use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use basedrop::Shared;
use fnv::FnvHashMap;

use crate::schedule::tasks::DelayCompNode;

#[derive(Clone)]
pub(crate) struct SharedDelayCompNode {
    pub active: bool,
    pub delay: u32,

    shared: Shared<AtomicRefCell<DelayCompNode>>,
}

impl SharedDelayCompNode {
    pub fn new(d: DelayCompNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            active: true,
            delay: d.delay(),
            shared: Shared::new(coll_handle, AtomicRefCell::new(d)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, DelayCompNode> {
        self.shared.borrow_mut()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DelayCompKey {
    pub src_node_ref: usize,
    pub port_stable_id: u32,
    pub port_channel_index: u16,
    pub delay: u32,
}

pub(crate) struct DelayCompNodePool {
    pub pool: FnvHashMap<DelayCompKey, SharedDelayCompNode>,
}

impl DelayCompNodePool {
    pub fn new() -> Self {
        Self { pool: FnvHashMap::default() }
    }
}
