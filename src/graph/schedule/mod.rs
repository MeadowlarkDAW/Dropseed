use basedrop::{Shared, SharedCell};
use maybe_atomic_refcell::MaybeAtomicRefCell;
use smallvec::SmallVec;
use std::thread::ThreadId;

use crate::thread_id::SharedThreadIDs;
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
}

impl Schedule {
    pub(crate) fn empty(max_block_size: usize) -> Self {
        Self {
            tasks: Vec::new(),
            graph_audio_in: SmallVec::new(),
            graph_audio_out: SmallVec::new(),
            max_block_size,
        }
    }
}

impl std::fmt::Debug for Schedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();

        s.push_str("Schedule {\n");

        let mut g_s = String::new();
        for b in self.graph_audio_in.iter() {
            g_s.push_str(&format!("{:?}, ", b.id()))
        }
        s.push_str(format!("    graph_audio_in: {:?},\n", &g_s).as_str());

        for t in self.tasks.iter() {
            s.push_str(format!("    {:?},\n", t).as_str());
        }

        let mut g_s = String::new();
        for b in self.graph_audio_out.iter() {
            g_s.push_str(&format!("{:?}, ", b.id()))
        }
        s.push_str(format!("    graph_audio_out: {:?},\n}}", &g_s).as_str());

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
                unsafe {
                    buffer.clear(proc_info.frames);
                }
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
                    let mut buffer_ref = unsafe { buffer.borrow_mut() };

                    #[cfg(debug_assertions)]
                    let buffer = &mut buffer_ref[0..proc_info.frames];
                    #[cfg(not(debug_assertions))]
                    let buffer = unsafe {
                        std::slice::from_raw_parts_mut(buffer_ref.as_mut_ptr(), proc_info.frames)
                    };

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
    schedule: MaybeAtomicRefCell<Schedule>,
}

// Required because of basedrop's `SharedCell` container.
//
// The reason why rust flags this as unsafe is because of the `MaybeAtomicRefCell`s in
// the schedule. But this is safe because those `MaybeAtomicRefCell`s only ever get
// borrowed in the audio thread.
unsafe impl Send for ScheduleWrapper {}
unsafe impl Sync for ScheduleWrapper {}

pub struct SharedSchedule {
    schedule: Shared<SharedCell<ScheduleWrapper>>,
    thread_ids: SharedThreadIDs,
    current_audio_thread_id: Option<ThreadId>,
    coll_handle: basedrop::Handle,
}

// Implement Debug so we can send it in an event.
impl std::fmt::Debug for SharedSchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedSchedule")
    }
}

impl SharedSchedule {
    pub(crate) fn new(
        schedule: Schedule,
        thread_ids: SharedThreadIDs,
        coll_handle: &basedrop::Handle,
    ) -> (Self, Self) {
        let schedule = Shared::new(
            coll_handle,
            SharedCell::new(Shared::new(
                coll_handle,
                ScheduleWrapper { schedule: MaybeAtomicRefCell::new(schedule) },
            )),
        );

        (
            Self {
                schedule: schedule.clone(),
                thread_ids: thread_ids.clone(),
                current_audio_thread_id: None,
                coll_handle: coll_handle.clone(),
            },
            Self {
                schedule,
                thread_ids,
                current_audio_thread_id: None,
                coll_handle: coll_handle.clone(),
            },
        )
    }

    pub(crate) fn set_new_schedule(&mut self, schedule: Schedule, coll_handle: &basedrop::Handle) {
        self.schedule.set(Shared::new(
            coll_handle,
            ScheduleWrapper { schedule: MaybeAtomicRefCell::new(schedule) },
        ));
    }

    #[cfg(feature = "cpal-backend")]
    pub fn process_cpal_interleaved_output_only<T: cpal::Sample>(
        &mut self,
        num_out_channels: usize,
        out: &mut [T],
    ) {
        let latest_schedule = self.schedule.get();

        // This is safe because the schedule is only ever accessed by the
        // audio thread.
        let mut schedule = unsafe { latest_schedule.schedule.borrow_mut() };

        // TODO: Set this in the sandbox thread once we implement plugin sandboxing.
        // Make sure the the audio thread ID is correct.
        if let Some(audio_thread_id) = self.thread_ids.external_audio_thread_id() {
            if std::thread::current().id() != audio_thread_id {
                self.thread_ids
                    .set_external_audio_thread_id(std::thread::current().id(), &self.coll_handle);
            }
        } else {
            self.thread_ids
                .set_external_audio_thread_id(std::thread::current().id(), &self.coll_handle);
        }

        schedule.process_cpal_interleaved_output_only(num_out_channels, out);
    }
}
