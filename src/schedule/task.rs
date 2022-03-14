use basedrop::Shared;
use clap_sys::process::clap_process;

use super::plugin_pool::SharedPluginAudioThreadInstance;
use crate::{
    host::Host,
    process_info::{ClapProcAudioPorts, ProcAudioBuffers, ProcInfo, ProcessStatus},
};

pub(crate) enum Task {
    InternalProcessor(InternalPluginTask),
    ClapProcessor(ClapPluginTask),
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::InternalProcessor(t) => {
                let mut f = f.debug_struct("InternalPlugin");

                f.field("id", &t.plugin.id());

                t.audio_buffers.debug_fields(&mut f);

                f.finish()
            }
            Task::ClapProcessor(t) => {
                let mut f = f.debug_struct("ClapPlugin");

                // TODO: Processor ID

                t.ports.debug_fields(&mut f);

                f.finish()
            }
        }
    }
}

pub(crate) struct InternalPluginTask {
    plugin: SharedPluginAudioThreadInstance,

    audio_buffers: ProcAudioBuffers,
}

impl InternalPluginTask {
    #[inline]
    pub fn process(&mut self, info: &ProcInfo, host: &mut Host) -> ProcessStatus {
        let Self { plugin, audio_buffers } = self;

        // This is safe because the audio thread counterpart of a plugin is only ever
        // borrowed mutably in this method. Also, the verifier has verified that no
        // data races exist between parallel audio threads (once we actually have
        // multi-threaded schedules of course).
        let plugin = unsafe { plugin.borrow_mut() };

        // Prepare the host handle to accept requests from the plugin.
        host.current_plugin_channel = Shared::clone(&plugin.channel);

        // TODO: input event stuff

        let status = if let Err(_) = plugin.plugin.start_processing(host) {
            ProcessStatus::Error
        } else {
            let status = plugin.plugin.process(info, audio_buffers, host);

            plugin.plugin.stop_processing(host);

            status
        };

        if let ProcessStatus::Error = status {
            // As per the spec, we must clear all output buffers.
            audio_buffers.clear_all_outputs(info);
        }

        // TODO: output event stuff

        status
    }
}

pub(crate) struct ClapPluginTask {
    // TODO: clap processor
    ports: ClapProcAudioPorts,
}

impl ClapPluginTask {
    #[inline]
    pub fn process(&mut self, proc: &mut clap_process) -> ProcessStatus {
        // Prepare the buffers to be sent to the external plugin.
        self.ports.prepare(proc);

        // TODO: process clap plugin

        todo!()
    }
}
