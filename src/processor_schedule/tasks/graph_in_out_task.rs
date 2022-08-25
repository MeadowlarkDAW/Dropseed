use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;
use smallvec::SmallVec;

pub(crate) struct GraphInTask {
    pub audio_out: SmallVec<[SharedBuffer<f32>; 4]>,
}

impl GraphInTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO: Collect inputs from audio thread.

        for shared_buffer in self.audio_out.iter() {
            shared_buffer.clear_until(proc_info.frames);
        }
    }
}

pub(crate) struct GraphOutTask {
    pub audio_in: SmallVec<[SharedBuffer<f32>; 4]>,
}

impl GraphOutTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO: Send outputs to audio thread.
    }
}
