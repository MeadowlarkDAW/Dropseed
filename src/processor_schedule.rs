use dropseed_plugin_api::ProcInfo;

pub(crate) mod tasks;

pub use tasks::TransportHandle;

use crate::graph::shared_pools::SharedTransportTask;

use tasks::{GraphInTask, GraphOutTask, Task};

pub struct ProcessorSchedule {
    tasks: Vec<Task>,

    graph_in_task: GraphInTask,
    graph_out_task: GraphOutTask,
    transport_task: SharedTransportTask,

    max_block_size: usize,
}

impl ProcessorSchedule {
    pub(crate) fn new(
        tasks: Vec<Task>,
        graph_in_task: GraphInTask,
        graph_out_task: GraphOutTask,
        transport_task: SharedTransportTask,
        max_block_size: usize,
    ) -> Self {
        Self { tasks, graph_in_task, graph_out_task, transport_task, max_block_size }
    }

    pub(crate) fn new_empty(max_block_size: usize, transport_task: SharedTransportTask) -> Self {
        Self {
            tasks: Vec::new(),
            graph_in_task: GraphInTask::default(),
            graph_out_task: GraphOutTask::default(),
            transport_task,
            max_block_size,
        }
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
    pub fn process_interleaved(&mut self, audio_in: &[f32], audio_out: &mut [f32]) {
        let audio_in_channels = self.graph_in_task.audio_in.len();
        let audio_out_channels = self.graph_out_task.audio_out.len();

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

            // De-interlace the audio in stream to the graph input buffers.
            for (channel_i, buffer) in self.graph_in_task.audio_in.iter().enumerate() {
                let buffer = &mut buffer.borrow_mut()[0..frames];

                // TODO: Check that the compiler is properly eliding bounds checking.
                for i in 0..frames {
                    buffer[i] = audio_in[((i + processed_frames) * audio_in_channels) + channel_i];
                }
            }

            let transport = self.transport_task.borrow_mut().process(frames);

            let proc_info = ProcInfo {
                steady_time: -1, // TODO
                frames,
                transport,
            };

            for task in self.tasks.iter_mut() {
                task.process(&proc_info)
            }

            // Interlace the graph output buffers to the audio out stream.
            for (channel_i, buffer) in self.graph_out_task.audio_out.iter().enumerate() {
                let buffer = &buffer.borrow()[0..frames];

                // TODO: Check that the compiler is properly eliding bounds checking.
                for i in 0..frames {
                    audio_out[((i + processed_frames) * audio_out_channels) + channel_i] =
                        buffer[i];
                }
            }

            processed_frames += frames;
        }
    }
}
