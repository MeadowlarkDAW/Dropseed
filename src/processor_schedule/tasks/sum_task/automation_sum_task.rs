use smallvec::SmallVec;

use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::event_io_buffers::AutomationIoEvent;

pub(crate) struct AutomationSumTask {
    pub input: SmallVec<[SharedBuffer<AutomationIoEvent>; 4]>,
    pub output: SharedBuffer<AutomationIoEvent>,
}

impl AutomationSumTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO
    }
}
