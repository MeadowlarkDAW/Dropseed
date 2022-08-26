use dropseed_plugin_api::ProcInfo;

pub(crate) mod tasks;

pub use tasks::TransportHandle;

use crate::graph::shared_pools::SharedTransportTask;

use tasks::Task;

pub struct ProcessorSchedule {
    tasks: Vec<Task>,

    transport_task: SharedTransportTask,

    max_block_size: usize,
}

impl ProcessorSchedule {
    pub(crate) fn new(
        tasks: Vec<Task>,
        transport_task: SharedTransportTask,
        max_block_size: usize,
    ) -> Self {
        Self { tasks, transport_task, max_block_size }
    }

    pub(crate) fn new_empty(max_block_size: usize, transport_task: SharedTransportTask) -> Self {
        Self { tasks: Vec::new(), transport_task, max_block_size }
    }

    pub(crate) fn tasks(&self) -> &[Task] {
        &self.tasks
    }
}

impl std::fmt::Debug for ProcessorSchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();

        s.push_str("ProcessorSchedule {\n");

        for t in self.tasks.iter() {
            s.push_str(format!("    {:?},\n", t).as_str());
        }

        write!(f, "{}", s)
    }
}

impl ProcessorSchedule {
    pub fn process_interleaved(
        &mut self,
        audio_in: &[f32],
        audio_in_channels: usize,
        audio_out: &mut [f32],
        audio_out_channels: usize,
    ) {
        if audio_in_channels != 0 && audio_out_channels != 0 {
            assert_eq!(audio_in.len() / audio_in_channels, audio_out.len() / audio_out_channels);
        }

        let total_frames = if audio_in_channels > 0 {
            let total_frames = audio_in.len() / audio_in_channels;

            assert_eq!(audio_out.len(), audio_out_channels * total_frames);

            total_frames
        } else if audio_out_channels > 0 {
            audio_out.len() / audio_out_channels
        } else {
            return;
        };

        if total_frames == 0 {
            return;
        }

        let mut processed_frames = 0;
        while processed_frames < total_frames {
            let frames = (total_frames - processed_frames).min(self.max_block_size);

            let transport = self.transport_task.borrow_mut().process(frames);

            let proc_info = ProcInfo {
                steady_time: -1, // TODO
                frames,
                transport,
            };

            for task in self.tasks.iter_mut() {
                task.process(&proc_info)
            }

            processed_frames += frames;
        }
    }
}
