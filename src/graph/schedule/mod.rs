use std::cell::UnsafeCell;

use basedrop::{Shared, SharedCell};
use smallvec::SmallVec;

use crate::host_request::HostInfo;
use crate::ProcInfo;

use super::shared_pool::SharedBuffer;

pub(crate) mod delay_comp_node;
pub(crate) mod sum;
pub(crate) mod task;

use task::Task;

pub struct Schedule {
    pub(crate) tasks: Vec<Task>,

    pub(crate) graph_audio_in: SmallVec<[SharedBuffer<f32>; 4]>,
    pub(crate) graph_audio_out: SmallVec<[SharedBuffer<f32>; 4]>,

    pub(crate) max_block_size: usize,

    /// Used to get info and request actions from the host.
    pub(crate) host_info: Shared<HostInfo>,
}

impl Schedule {
    pub(crate) fn empty(max_block_size: usize, host_info: Shared<HostInfo>) -> Self {
        Self {
            tasks: Vec::new(),
            graph_audio_in: SmallVec::new(),
            graph_audio_out: SmallVec::new(),
            max_block_size,
            host_info,
        }
    }
}

impl std::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();

        s.push_str("Schedule {\n    tasks:\n");

        for t in self.tasks.iter() {
            s.push_str(format!("        {:?},\n", t).as_str());
        }

        let mut g_s = String::new();
        for b in self.graph_audio_in.iter() {
            g_s.push_str(&format!("{:?}, ", b.id()))
        }
        s.push_str(format!("\n    graph_audio_in: {:?},\n", &g_s).as_str());

        let mut g_s = String::new();
        for b in self.graph_audio_out.iter() {
            g_s.push_str(&format!("{:?}, ", b.id()))
        }
        s.push_str(format!("    graph_audio_out: {:?},\n", &g_s).as_str());

        s.push_str(format!("    max_block_size: {},\n", &self.max_block_size).as_str());
        s.push_str(format!("    host_info: {:?},\n}}", &*self.host_info).as_str());

        write!(f, "{}", s)
    }
}

impl Schedule {
    #[cfg(feature = "cpal-backend")]
    pub fn process_cpal_interleaved_output_only<T: cpal::Sample>(
        &mut self,
        num_out_channels: usize,
        out: &mut [T],
    ) {
        if num_out_channels == 0 || out.is_empty() {
            for smp in out.iter_mut() {
                *smp = T::from(&0.0);
            }

            return;
        }

        // Get the number of frames in this process cycle
        let total_frames = out.len() / num_out_channels;

        if total_frames * num_out_channels != out.len() {
            log::warn!("The given cpal output buffer with {} total samples is not a multiple of {} channels", out.len(), num_out_channels);
            for smp in out[(total_frames * num_out_channels)..].iter_mut() {
                *smp = T::from(&0.0);
            }
        }

        let mut processed_frames = 0;
        while processed_frames < total_frames {
            let frames = (total_frames - processed_frames).min(self.max_block_size);

            let proc_info = ProcInfo {
                steady_time: -1, // TODO
                frames,
            };

            // We are ignoring sytem inputs with the CPAL backend for now.
            for buffer in self.graph_audio_in.iter() {
                // SAFETY
                // - These buffers are only ever borrowed in the audio thread.
                // - The schedule verifier has ensured that no data races can occur between parallel
                // audio threads due to aliasing buffer pointers.
                // - `proc_info.frames` will always be less than or equal to the allocated size of
                // all process audio buffers.
                let buffer = unsafe { buffer.slice_from_frames_unchecked_mut(proc_info.frames) };

                buffer.fill(0.0);
            }

            for task in self.tasks.iter_mut() {
                task.process(&proc_info)
            }

            let out_part = &mut out[(processed_frames * num_out_channels)
                ..((processed_frames + frames) * num_out_channels)];
            for ch_i in 0..num_out_channels {
                if let Some(buffer) = self.graph_audio_out.get(ch_i) {
                    // SAFETY
                    // - These buffers are only ever borrowed in the audio thread.
                    // - The schedule verifier has ensured that no data races can occur between parallel
                    // audio threads due to aliasing buffer pointers.
                    // - `proc_info.frames` will always be less than or equal to the allocated size of
                    // all process audio buffers.
                    let buffer =
                        unsafe { buffer.slice_from_frames_unchecked_mut(proc_info.frames) };

                    for i in 0..frames {
                        // TODO: Optimize with unsafe bounds checking?
                        out_part[(i * num_out_channels) + ch_i] = T::from(&buffer[i]);
                    }
                } else {
                    for i in 0..frames {
                        // TODO: Optimize with unsafe bounds checking?
                        out_part[(i * num_out_channels) + ch_i] = T::from(&0.0);
                    }
                }
            }

            processed_frames += frames;
        }
    }
}

struct ScheduleWrapper {
    schedule: UnsafeCell<Schedule>,
}

// Required because of basedrop's `SharedCell` container.
//
// The reason why rust flags this as unsafe is because of the `UnsafeCell`s in
// the schedule. But this is safe because those `UnsafeCell`s only ever get
// borrowed in the audio thread.
unsafe impl Send for ScheduleWrapper {}
unsafe impl Sync for ScheduleWrapper {}

pub struct SharedSchedule {
    schedule: Shared<SharedCell<ScheduleWrapper>>,
}

// Implement Debug so we can send it in an event.
impl std::fmt::Debug for SharedSchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedSchedule")
    }
}

impl SharedSchedule {
    pub(crate) fn new(schedule: Schedule, coll_handle: &basedrop::Handle) -> (Self, Self) {
        let schedule = Shared::new(
            coll_handle,
            SharedCell::new(Shared::new(
                coll_handle,
                ScheduleWrapper { schedule: UnsafeCell::new(schedule) },
            )),
        );

        (Self { schedule: schedule.clone() }, Self { schedule })
    }

    pub(crate) fn set_new_schedule(&mut self, schedule: Schedule, coll_handle: &basedrop::Handle) {
        self.schedule
            .set(Shared::new(coll_handle, ScheduleWrapper { schedule: UnsafeCell::new(schedule) }));
    }

    #[cfg(feature = "cpal-backend")]
    pub fn process_cpal_interleaved_output_only<T: cpal::Sample>(
        &mut self,
        num_out_channels: usize,
        out: &mut [T],
    ) {
        // This is safe because the schedule is only ever accessed by the
        // audio thread.
        let schedule = unsafe { &mut *(*self.schedule.get()).schedule.get() };

        schedule.process_cpal_interleaved_output_only(num_out_channels, out);
    }
}
