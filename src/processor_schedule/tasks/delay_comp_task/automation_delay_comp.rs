use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::{
    graph::shared_pools::SharedAutomationDelayCompNode,
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
