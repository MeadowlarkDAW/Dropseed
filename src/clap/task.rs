use crate::plugin::process_info::ProcessStatus;

use super::process::ClapProcess;

pub(crate) struct ClapPluginTask {
    // TODO: clap processor
//ports: ClapProcAudioPorts,
}

impl ClapPluginTask {
    fn process(&mut self, proc: &mut ClapProcess) {
        // Prepare the buffers to be sent to the external plugin.
        //self.ports.prepare(proc);

        // TODO: process clap plugin

        todo!()
    }
}
