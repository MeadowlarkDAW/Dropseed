use basedrop::Shared;
use dropseed_core::RtGCHandle;
use std::cell::UnsafeCell;

use super::{descriptor::NodeType, NodeAudioThr};

pub(crate) struct NodeHostAudioThr {
    node: Box<dyn NodeAudioThr>,

    node_type: NodeType,
}

impl NodeHostAudioThr {
    pub fn new(node: Box<dyn NodeAudioThr>, node_type: NodeType) -> Self {
        Self { node, node_type }
    }

    pub fn process(&mut self) {}
}

pub(crate) struct SharedNodeHostAudioThr {
    pub shared: Shared<UnsafeCell<NodeHostAudioThr>>,
}

impl SharedNodeHostAudioThr {
    pub fn new(h: NodeHostAudioThr, gc: &RtGCHandle) -> Self {
        Self { shared: Shared::new(gc.handle(), UnsafeCell::new(h)) }
    }
}

impl Clone for SharedNodeHostAudioThr {
    fn clone(&self) -> Self {
        Self { shared: Shared::clone(&self.shared) }
    }
}
