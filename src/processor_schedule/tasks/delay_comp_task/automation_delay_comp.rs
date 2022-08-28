use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;
use basedrop::Shared;
use atomic_refcell::{AtomicRefCell, AtomicRefMut};

use crate::{
    plugin_host::event_io_buffers::AutomationIoEvent,
};

pub(crate) struct AutomationDelayCompTask {
    pub shared_node: SharedAutomationDelayCompNode,

    pub input: SharedBuffer<AutomationIoEvent>,
    pub output: SharedBuffer<AutomationIoEvent>,
}

impl AutomationDelayCompTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let mut delay_comp_node = self.shared_node.borrow_mut();

        delay_comp_node.process(proc_info, &self.input, &self.output);
    }
}

#[derive(Clone)]
pub(crate) struct SharedAutomationDelayCompNode {
    pub active: bool,
    pub delay: u32,

    shared: Shared<AtomicRefCell<AutomationDelayCompNode>>,
}

impl SharedAutomationDelayCompNode {
    pub fn new(d: AutomationDelayCompNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            active: true,
            delay: d.delay(),
            shared: Shared::new(coll_handle, AtomicRefCell::new(d)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, AutomationDelayCompNode> {
        self.shared.borrow_mut()
    }
}

pub(crate) struct AutomationDelayCompNode {
    // TODO
    delay: u32,
}

impl AutomationDelayCompNode {
    pub fn new(delay: u32) -> Self {
        Self { delay }
    }

    pub fn process(
        &mut self,
        proc_info: &ProcInfo,
        input: &SharedBuffer<AutomationIoEvent>,
        output: &SharedBuffer<AutomationIoEvent>,
    ) {
        // TODO
    }

    pub fn delay(&self) -> u32 {
        self.delay
    }
}
