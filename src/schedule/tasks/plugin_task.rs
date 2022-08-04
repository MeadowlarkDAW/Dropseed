use dropseed_plugin_api::ProcInfo;
use dropseed_plugin_api::{PluginInstanceID, ProcBuffers};

use crate::plugin_host::events::PluginEventIoBuffers;
use crate::plugin_host::SharedPluginHostProcThread;

pub(crate) struct PluginTask {
    pub plugin_id: PluginInstanceID,
    pub shared_processor: SharedPluginHostProcThread,

    pub buffers: ProcBuffers,

    pub event_buffers: PluginEventIoBuffers,
}

impl PluginTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let mut processor = self.shared_processor.borrow_mut();

        processor.process(proc_info, &mut self.buffers, &mut self.event_buffers);
    }
}
