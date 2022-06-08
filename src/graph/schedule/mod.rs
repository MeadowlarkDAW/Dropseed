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
    pub fn process_interleaved(
        &mut self,
        audio_in: &[f32],
        audio_in_channels: usize,
        audio_out: &mut [f32],
        audio_out_channels: usize,
    ) {
        if audio_in_channels != 0 && audio_out_channels != 0 {
            debug_assert_eq!(
                audio_in.len() / audio_in_channels,
                audio_out.len() / audio_out_channels
            );
        }

        let total_frames = if audio_in_channels > 0 {
            let total_frames = audio_in.len() / audio_in_channels;

            debug_assert_eq!(audio_out.len(), audio_out_channels * total_frames);

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

            let proc_info = ProcInfo {
                steady_time: -1, // TODO
                frames,
            };

            for (ch_i, in_buffer) in self.graph_audio_in.iter().enumerate() {
                if ch_i < audio_in_channels {
                    // SAFETY
                    // - These buffers are only ever borrowed in the audio thread.
                    // - The schedule verifier has ensured that no data races can occur between parallel
                    // audio threads due to aliasing buffer pointers.
                    // - `frames` will always be less than or equal to the allocated size of
                    // all process audio buffers.
                    let mut buffer_ref = unsafe { in_buffer.borrow_mut() };

                    #[cfg(debug_assertions)]
                    let buffer = &mut buffer_ref[0..frames];
                    #[cfg(not(debug_assertions))]
                    let buffer =
                        unsafe { std::slice::from_raw_parts_mut(buffer_ref.as_mut_ptr(), frames) };

                    for i in 0..proc_info.frames {
                        #[cfg(debug_assertions)]
                        {
                            buffer[i] = audio_in[(i * audio_in_channels) + ch_i];
                        }

                        #[cfg(not(debug_assertions))]
                        unsafe {
                            *buffer.get_unchecked_mut(i) =
                                *audio_in.get_unchecked((i * audio_in_channels) + ch_i);
                        }
                    }

                    let mut is_constant = true;
                    let first_val = buffer[0];
                    for i in 0..frames {
                        if buffer[i] != first_val {
                            is_constant = false;
                            break;
                        }
                    }

                    in_buffer.set_constant(is_constant);
                } else {
                    unsafe {
                        in_buffer.clear(frames);
                    }
                }
            }

            for task in self.tasks.iter_mut() {
                task.process(&proc_info)
            }

            let out_part = &mut audio_out[(processed_frames * audio_out_channels)
                ..((processed_frames + frames) * audio_out_channels)];
            for ch_i in 0..audio_out_channels {
                if let Some(buffer) = self.graph_audio_out.get(ch_i) {
                    // SAFETY
                    // - These buffers are only ever borrowed in the audio thread.
                    // - The schedule verifier has ensured that no data races can occur between parallel
                    // audio threads due to aliasing buffer pointers.
                    // - `frames` will always be less than or equal to the allocated size of
                    // all process audio buffers.
                    let mut buffer_ref = unsafe { buffer.borrow_mut() };

                    #[cfg(debug_assertions)]
                    let buffer = &mut buffer_ref[0..frames];
                    #[cfg(not(debug_assertions))]
                    let buffer =
                        unsafe { std::slice::from_raw_parts_mut(buffer_ref.as_mut_ptr(), frames) };

                    for i in 0..frames {
                        #[cfg(debug_assertions)]
                        {
                            out_part[(i * audio_out_channels) + ch_i] = buffer[i];
                        }

                        #[cfg(not(debug_assertions))]
                        unsafe {
                            *out_part.get_unchecked_mut((i * audio_out_channels) + ch_i) =
                                *buffer.get_unchecked(i);
                        }
                    }
                } else {
                    for i in 0..frames {
                        #[cfg(debug_assertions)]
                        {
                            out_part[(i * audio_out_channels) + ch_i] = 0.0;
                        }

                        #[cfg(not(debug_assertions))]
                        unsafe {
                            *out_part.get_unchecked_mut((i * audio_out_channels) + ch_i) = 0.0;
                        }
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

pub(crate) struct SharedSchedule {
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

    pub(crate) fn process_interleaved(
        &mut self,
        audio_in: &[f32],
        audio_in_channels: usize,
        audio_out: &mut [f32],
        audio_out_channels: usize,
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

        schedule.process_interleaved(audio_in, audio_in_channels, audio_out, audio_out_channels);
    }
}
