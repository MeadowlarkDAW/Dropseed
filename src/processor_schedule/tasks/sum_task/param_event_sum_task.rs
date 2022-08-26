use smallvec::SmallVec;

use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::event_io_buffers::ParamIoEvent;

pub(crate) struct ParamEventSumTask {
    pub event_in: SmallVec<[SharedBuffer<ParamIoEvent>; 4]>,
    pub event_out: SharedBuffer<ParamIoEvent>,
}

impl ParamEventSumTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO
    }
}
