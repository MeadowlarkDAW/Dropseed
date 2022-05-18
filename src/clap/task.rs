use crate::ProcInfo;

use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_SLEEP,
    CLAP_PROCESS_TAIL,
};

use super::{plugin::ClapPluginAudioThread, process::ClapProcess};
use crate::graph::plugin_pool::ProcessingState;

pub(crate) struct ClapPluginTask {
    pub plugin: ClapPluginAudioThread,

    pub clap_process: ClapProcess,
}

impl ClapPluginTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let clear_outputs = |clap_process: &mut ClapProcess| {
            for b in clap_process.audio_out.iter_mut() {
                b.clear(proc_info);
            }
        };

        let state = self.plugin.processing_state();

        let processing_requested = self.plugin.processing_requested();

        if self.plugin.deactivation_requested() && state != ProcessingState::Sleeping {
            self.plugin.set_processing_state(ProcessingState::Sleeping);
            self.plugin.stop_processing();

            clear_outputs(&mut self.clap_process);
            return;
        }

        if let ProcessingState::Sleeping = state {
            let has_input_event = true; // TODO

            if processing_requested || has_input_event {
                if self.plugin.start_processing() {
                    self.plugin.set_processing_state(ProcessingState::Processing);
                } else {
                    self.plugin.set_processing_state(ProcessingState::Sleeping);

                    clear_outputs(&mut self.clap_process);
                    return;
                }
            } else {
                clear_outputs(&mut self.clap_process);
                return;
            }
        }

        if let ProcessingState::WaitingForQuietToSleep = state {
            let mut is_silent = true;
            for b in self.clap_process.audio_in.iter() {
                if !b.is_silent(proc_info) {
                    is_silent = false;
                    break;
                }
            }

            if is_silent {
                self.plugin.set_processing_state(ProcessingState::Sleeping);
                self.plugin.stop_processing();

                clear_outputs(&mut self.clap_process);
                return;
            }
        }

        if let ProcessingState::WaitingForTailToSleep = state {
            // TODO
        }

        // TODO: input event stuff

        self.clap_process.update_frames(proc_info);

        let status = self.plugin.process(self.clap_process.raw());

        // TODO: output event stuff

        match status {
            CLAP_PROCESS_CONTINUE => {
                self.plugin.set_processing_state(ProcessingState::Processing);
            }
            CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => {
                self.plugin.set_processing_state(ProcessingState::WaitingForQuietToSleep);
            }
            CLAP_PROCESS_TAIL => {
                self.plugin.set_processing_state(ProcessingState::WaitingForTailToSleep);
            }
            CLAP_PROCESS_SLEEP => {
                if state != ProcessingState::Sleeping {
                    self.plugin.set_processing_state(ProcessingState::Sleeping);
                    self.plugin.stop_processing();
                }
            }
            _ => {
                clear_outputs(&mut self.clap_process);
            }
        }
    }
}
