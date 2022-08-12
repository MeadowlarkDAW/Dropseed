use dropseed_plugin_api::buffer::{DebugBufferID, RawAudioChannelBuffers};
use fnv::FnvHashSet;

use crate::processor_schedule::{tasks::Task, ProcessorSchedule};

use super::super::error::VerifyScheduleError;

pub(crate) struct Verifier {
    plugin_instances: FnvHashSet<u64>,
    buffer_instances: FnvHashSet<DebugBufferID>,
}

impl Verifier {
    pub fn new() -> Self {
        let mut plugin_instances: FnvHashSet<u64> = FnvHashSet::default();
        let mut buffer_instances: FnvHashSet<DebugBufferID> = FnvHashSet::default();
        plugin_instances.reserve(1024);
        buffer_instances.reserve(1024);

        Verifier { plugin_instances, buffer_instances }
    }

    /// This is probably expensive, but I would like to keep this check here until we are very
    /// confident in the stability and soundness of this audio graph compiler.
    ///
    /// We are using reference-counted pointers (`basedrop::Shared`) for everything, so we shouldn't
    /// ever run into a situation where the schedule assigns a pointer to a buffer or a node that
    /// doesn't exist in memory.
    ///
    /// However, it is still very possible to have race condition bugs in the schedule, such as
    /// the same buffer being assigned multiple times within the same task, or the same buffer
    /// appearing multiple times between parallel tasks (once we have multithreaded scheduling).
    pub fn verify_schedule_for_race_conditions(
        &mut self,
        schedule: &ProcessorSchedule,
    ) -> Result<(), VerifyScheduleError> {
        // TODO: verifying that there are not data races between parallel threads once we
        // have multithreaded scheduling.

        self.plugin_instances.clear();

        for task in schedule.tasks().iter() {
            self.buffer_instances.clear();

            match task {
                Task::Plugin(t) => {
                    if !self.plugin_instances.insert(t.plugin_id.unique_id()) {
                        return Err(VerifyScheduleError::PluginInstanceAppearsTwiceInSchedule {
                            plugin_id: t.plugin_id.clone(),
                        });
                    }

                    for port_buffer in t.buffers.audio_in.iter() {
                        match &port_buffer._raw_channels {
                            RawAudioChannelBuffers::F32(buffers) => {
                                for b in buffers.iter() {
                                    if !self.buffer_instances.insert(b.id()) {
                                        return Err(
                                            VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                                buffer_id: b.id(),
                                                task_info: format!("{:?}", &task),
                                            },
                                        );
                                    }
                                }
                            }
                            RawAudioChannelBuffers::F64(buffers) => {
                                for b in buffers.iter() {
                                    if !self.buffer_instances.insert(b.id()) {
                                        return Err(
                                            VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                                buffer_id: b.id(),
                                                task_info: format!("{:?}", &task),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }

                    for port_buffer in t.buffers.audio_out.iter() {
                        match &port_buffer._raw_channels {
                            RawAudioChannelBuffers::F32(buffers) => {
                                for b in buffers.iter() {
                                    if !self.buffer_instances.insert(b.id()) {
                                        return Err(
                                            VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                                buffer_id: b.id(),
                                                task_info: format!("{:?}", &task),
                                            },
                                        );
                                    }
                                }
                            }
                            RawAudioChannelBuffers::F64(buffers) => {
                                for b in buffers.iter() {
                                    if !self.buffer_instances.insert(b.id()) {
                                        return Err(
                                            VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                                buffer_id: b.id(),
                                                task_info: format!("{:?}", &task),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Task::DelayComp(t) => {
                    if t.audio_in.id() == t.audio_out.id() {
                        return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                            buffer_id: t.audio_in.id(),
                            task_info: format!("{:?}", &task),
                        });
                    }
                }
                Task::Sum(t) => {
                    // This could be made just a warning and not an error, but it's still not what
                    // we want to happen.
                    if t.audio_in.len() < 2 {
                        return Err(VerifyScheduleError::SumNodeWithLessThanTwoInputs {
                            num_inputs: t.audio_in.len(),
                            task_info: format!("{:?}", &task),
                        });
                    }

                    // This verification step is probably overkill because I don't believe
                    // that simply summing aliased buffers can lead to a race condition issue.
                    // Unless the compiler tries to copy some of those buffers and expects those
                    // buffers to not alias or something.
                    for b in t.audio_in.iter() {
                        if !self.buffer_instances.insert(b.id()) {
                            return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                buffer_id: b.id(),
                                task_info: format!("{:?}", &task),
                            });
                        }
                    }
                    if !self.buffer_instances.insert(t.audio_out.id()) {
                        return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                            buffer_id: t.audio_out.id(),
                            task_info: format!("{:?}", &task),
                        });
                    }
                }
                Task::DeactivatedPlugin(t) => {
                    for (b_in, b_out) in t.audio_through.iter() {
                        if !self.buffer_instances.insert(b_in.id()) {
                            return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                buffer_id: b_in.id(),
                                task_info: format!("{:?}", &task),
                            });
                        }
                        if !self.buffer_instances.insert(b_out.id()) {
                            return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                buffer_id: b_out.id(),
                                task_info: format!("{:?}", &task),
                            });
                        }
                    }

                    for b in t.extra_audio_out.iter() {
                        if !self.buffer_instances.insert(b.id()) {
                            return Err(VerifyScheduleError::BufferAppearsTwiceInSameTask {
                                buffer_id: b.id(),
                                task_info: format!("{:?}", &task),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
