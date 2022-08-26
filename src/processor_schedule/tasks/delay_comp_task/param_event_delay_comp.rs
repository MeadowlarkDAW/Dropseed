use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::{
    graph::shared_pools::SharedParamEventDelayCompNode, plugin_host::event_io_buffers::ParamIoEvent,
};

pub(crate) struct ParamEventDelayCompTask {
    pub shared_node: SharedParamEventDelayCompNode,

    pub event_in: SharedBuffer<ParamIoEvent>,
    pub event_out: SharedBuffer<ParamIoEvent>,
}

impl ParamEventDelayCompTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let mut delay_comp_node = self.shared_node.borrow_mut();

        delay_comp_node.process(proc_info, &self.event_in, &self.event_out);
    }
}

pub(crate) struct ParamEventDelayCompNode {
    // TODO
    delay: u32,
}

impl ParamEventDelayCompNode {
    pub fn new(delay: u32) -> Self {
        Self { delay }
    }

    pub fn process(
        &mut self,
        proc_info: &ProcInfo,
        input: &SharedBuffer<ParamIoEvent>,
        output: &SharedBuffer<ParamIoEvent>,
    ) {
        // TODO
    }

    pub fn delay(&self) -> u32 {
        self.delay
    }
}
