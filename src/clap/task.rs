use crate::{plugin::process_info::ProcessStatus, ProcInfo};

use super::process::ClapProcess;

pub(crate) struct ClapPluginTask {
    clap_process: ClapProcess,
}

impl ClapPluginTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        self.clap_process.update_frames(proc_info);

        // TODO: process clap plugin

        todo!()
    }
}
