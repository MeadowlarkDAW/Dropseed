use basedrop::Shared;
use clap_sys::process::clap_process;
use smallvec::SmallVec;

use crate::graph::plugin_pool::SharedPluginAudioThreadInstance;
use crate::{host::Host, AudioPortBuffer, ProcInfo, ProcessStatus};

pub(crate) enum Task {
    InternalPlugin(InternalPluginTask),
    ClapPlugin(ClapPluginTask),
    DeactivatedPlugin(DeactivatedPluginTask),
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::InternalPlugin(t) => {
                let mut f = f.debug_struct("InternalPlugin");

                f.field("id", &t.plugin.id());

                if !t.audio_in.is_empty() {
                    let mut s = String::new();
                    for b in t.audio_in.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_in", &s);
                }

                if !t.audio_out.is_empty() {
                    let mut s = String::new();
                    for b in t.audio_out.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_out", &s);
                }

                f.finish()
            }
            Task::ClapPlugin(t) => {
                let mut f = f.debug_struct("ClapPlugin");

                // TODO: Processor ID

                //t.ports.debug_fields(&mut f);

                f.finish()
            }
            Task::DeactivatedPlugin(t) => {
                let mut f = f.debug_struct("DeactivatedPlugin");

                if !t.audio_out.is_empty() {
                    let mut s = String::new();
                    for b in t.audio_out.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_out", &s);
                }

                f.finish()
            }
        }
    }
}

impl Task {
    pub fn process(&mut self, info: &ProcInfo, host: &mut Host) -> ProcessStatus {
        match self {
            Task::InternalPlugin(task) => task.process(info, host),
            Task::ClapPlugin(task) => {
                todo!()
            }
            Task::DeactivatedPlugin(task) => task.process(info, host),
        }
    }
}

pub(crate) struct InternalPluginTask {
    plugin: SharedPluginAudioThreadInstance,

    audio_in: SmallVec<[AudioPortBuffer; 2]>,
    audio_out: SmallVec<[AudioPortBuffer; 2]>,
}

impl InternalPluginTask {
    fn process(&mut self, info: &ProcInfo, host: &mut Host) -> ProcessStatus {
        let Self { plugin, audio_in, audio_out } = self;

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
            let status = plugin.plugin.process(info, audio_in, audio_out, host);

            plugin.plugin.stop_processing(host);

            status
        };

        if let ProcessStatus::Error = status {
            // As per the spec, we must clear all output buffers.
            for b in audio_out.iter_mut() {
                b.clear(info);
            }
        }

        // TODO: output event stuff

        status
    }
}

pub(crate) struct DeactivatedPluginTask {
    audio_out: SmallVec<[AudioPortBuffer; 2]>,
}

impl DeactivatedPluginTask {
    fn process(&mut self, info: &ProcInfo, host: &mut Host) -> ProcessStatus {
        // Clear any output buffers.
        for b in self.audio_out.iter_mut() {
            b.clear(info);
        }

        ProcessStatus::Sleep
    }
}

pub(crate) struct ClapPluginTask {
    // TODO: clap processor
//ports: ClapProcAudioPorts,
}

impl ClapPluginTask {
    fn process(&mut self, proc: &mut clap_process) -> ProcessStatus {
        // Prepare the buffers to be sent to the external plugin.
        //self.ports.prepare(proc);

        // TODO: process clap plugin

        todo!()
    }
}
