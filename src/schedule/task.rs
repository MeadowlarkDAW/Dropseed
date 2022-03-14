use basedrop::Shared;
use clap_sys::process::clap_process;

use super::plugin_pool::SharedPluginAudioThreadInstance;
use crate::{
    host::Host,
    process_info::{ClapProcAudioPorts, ProcAudioBuffers, ProcInfo, ProcessStatus},
};

pub(crate) enum Task {
    InternalProcessor(InternalProcessorTask),
    ClapProcessor(ClapProcessorTask),
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::InternalProcessor(t) => {
                let mut f = f.debug_struct("IntProc");

                f.field("id", &t.plugin.id());

                t.audio_buffers.debug_fields(&mut f);

                f.finish()
            }
            Task::ClapProcessor(t) => {
                let mut f = f.debug_struct("ClapProc");

                // TODO: Processor ID

                t.ports.debug_fields(&mut f);

                f.finish()
            }
        }
    }
}

pub(crate) struct InternalProcessorTask {
    plugin: SharedPluginAudioThreadInstance,

    audio_buffers: ProcAudioBuffers,
}

impl InternalProcessorTask {
    #[inline]
    pub fn process(&mut self, info: &ProcInfo, host: &mut Host) {
        let Self { plugin, audio_buffers } = self;

        // This is safe because the audio thread counterpart of a plugin only ever
        // borrowed mutably here in the audio thread. Also, the verifier has verified
        // that no data races exist between parallel audio threads (once we actually
        // have multi-threaded schedules of course).
        let plugin = unsafe { plugin.borrow_mut() };

        // Prepare the host handle to accept requests from the plugin.
        host.current_plugin_channel = Shared::clone(&plugin.channel);

        // TODO: event stuff

        // TODO: process stuff

        // TODO: event stuff

        todo!()
    }
}

pub(crate) struct ClapProcessorTask {
    // TODO: clap processor
    ports: ClapProcAudioPorts,
}

impl ClapProcessorTask {
    #[inline]
    pub fn process(&mut self, proc: &mut clap_process) -> ProcessStatus {
        // Prepare the buffers to be sent to the external plugin.
        self.ports.prepare(proc);

        // TODO: process clap plugin

        todo!()
    }
}
