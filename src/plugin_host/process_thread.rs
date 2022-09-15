use clack_host::events::Event;
use dropseed_plugin_api::buffer::EventBuffer;
use dropseed_plugin_api::{PluginProcessThread, ProcBuffers, ProcInfo, ProcessStatus};

use crate::utils::thread_id::SharedThreadIDs;

use super::channel::{PlugHostChannelProcThread, PluginActiveState};
use super::event_io_buffers::{PluginEventIoBuffers, PluginEventOutputSanitizer};

#[derive(Copy, Clone, Debug, PartialEq)]
enum ProcessingState {
    WaitingForStart,
    Started(ProcessStatus),
    Stopped,
    Errored,
}

pub(crate) struct PluginHostProcThread {
    plugin: Box<dyn PluginProcessThread>,
    plugin_instance_id: u64,

    channel: PlugHostChannelProcThread,

    in_events: EventBuffer,
    out_events: EventBuffer,

    event_output_sanitizer: PluginEventOutputSanitizer,

    processing_state: ProcessingState,

    thread_ids: SharedThreadIDs,
}

impl PluginHostProcThread {
    pub(crate) fn new(
        plugin: Box<dyn PluginProcessThread>,
        plugin_instance_id: u64,
        channel: PlugHostChannelProcThread,
        num_params: usize,
        thread_ids: SharedThreadIDs,
    ) -> Self {
        Self {
            plugin,
            plugin_instance_id,
            channel,
            in_events: EventBuffer::with_capacity(num_params * 3),
            out_events: EventBuffer::with_capacity(num_params * 3),
            event_output_sanitizer: PluginEventOutputSanitizer::new(num_params),
            processing_state: ProcessingState::WaitingForStart,
            thread_ids,
        }
    }

    pub fn process(
        &mut self,
        proc_info: &ProcInfo,
        buffers: &mut ProcBuffers,
        event_buffers: &mut PluginEventIoBuffers,
    ) {
        // Always clear event and note output buffers.
        event_buffers.clear_before_process();

        let state = self.channel.shared_state.get_active_state();

        // Do we want to deactivate the plugin?
        if state == PluginActiveState::WaitingToDrop {
            if let ProcessingState::Started(_) = self.processing_state {
                self.plugin.stop_processing();
            }

            buffers.clear_all_outputs(proc_info);
            return;
        } else if self.channel.shared_state.should_start_processing() {
            if let ProcessingState::Started(_) = self.processing_state {
            } else {
                self.processing_state = ProcessingState::WaitingForStart;
            }
        }

        // We can't process a plugin which failed to start processing.
        if self.processing_state == ProcessingState::Errored {
            buffers.clear_all_outputs(proc_info);
            return;
        }

        // Reading in_events from all sources //

        self.in_events.clear();
        let mut has_param_in_event = self
            .channel
            .param_queues
            .as_mut()
            .map(|q| q.consume_into_event_buffer(&mut self.in_events))
            .unwrap_or(false);

        let (has_note_in_event, wrote_param_in_event) =
            event_buffers.write_input_events(&mut self.in_events, self.plugin_instance_id);

        has_param_in_event = has_param_in_event || wrote_param_in_event;

        if let Some(transport_in_event) = proc_info.transport.event() {
            self.in_events.push(transport_in_event.as_unknown());
        }

        // Check if inputs are quiet or not //

        if self.processing_state == ProcessingState::Started(ProcessStatus::ContinueIfNotQuiet)
            && !has_note_in_event
            && buffers.audio_inputs_silent(proc_info.frames)
            && buffers.audio_in.len() > 0
        {
            self.plugin.stop_processing();

            self.processing_state = ProcessingState::Stopped;
            buffers.clear_all_outputs(proc_info);

            if has_param_in_event {
                self.plugin.param_flush(&self.in_events, &mut self.out_events);
            }

            self.in_events.clear();
            return;
        }

        // Check if the plugin should be waking up //

        if let ProcessingState::Stopped | ProcessingState::WaitingForStart = self.processing_state {
            if self.processing_state == ProcessingState::Stopped && !has_note_in_event {
                // The plugin is sleeping, there is no request to wake it up, and there
                // are no events to process.
                buffers.clear_all_outputs(proc_info);

                if has_param_in_event {
                    self.plugin.param_flush(&self.in_events, &mut self.out_events);
                }

                self.in_events.clear();
                return;
            }

            if let Err(e) = self.plugin.start_processing() {
                log::error!("Plugin has failed to start processing: {}", e);

                // The plugin failed to start processing.
                self.processing_state = ProcessingState::Errored;
                buffers.clear_all_outputs(proc_info);

                if has_param_in_event {
                    self.plugin.param_flush(&self.in_events, &mut self.out_events);
                }

                return;
            }

            self.channel.shared_state.set_active_state(PluginActiveState::Active);
        }

        // Actual processing //

        self.out_events.clear();

        let new_status =
            if let Some(automation_out_buffer) = &mut event_buffers.automation_out_buffer {
                let automation_out_buffer = &mut *automation_out_buffer.borrow_mut();

                self.plugin.process_with_automation_out(
                    proc_info,
                    buffers,
                    &self.in_events,
                    &mut self.out_events,
                    automation_out_buffer,
                )
            } else {
                self.plugin.process(proc_info, buffers, &self.in_events, &mut self.out_events)
            };

        // Read from output events queue //

        if let Some(params_queue) = &mut self.channel.param_queues {
            params_queue.to_main_param_value_tx.produce(|mut producer| {
                event_buffers.read_output_events(
                    &self.out_events,
                    Some(&mut producer),
                    &mut self.event_output_sanitizer,
                    proc_info.frames as u32,
                )
            });
        } else {
            event_buffers.read_output_events(
                &self.out_events,
                None,
                &mut self.event_output_sanitizer,
                proc_info.frames as u32,
            );
        }

        // Update processing state //

        self.processing_state = match new_status {
            // ProcessStatus::Tail => TODO: handle tail by reading from the tail extension
            ProcessStatus::Sleep => {
                self.plugin.stop_processing();

                ProcessingState::Stopped
            }
            ProcessStatus::Error => {
                // Discard all output buffers.
                buffers.clear_all_outputs(proc_info);
                ProcessingState::Errored
            }
            good_status => ProcessingState::Started(good_status),
        };
    }

    pub fn stop_processing(&mut self) {
        if self.thread_ids.is_process_thread() {
            if let ProcessingState::Started(_) = self.processing_state {
                self.plugin.stop_processing();
            }
        }
    }
}

impl Drop for PluginHostProcThread {
    fn drop(&mut self) {
        if self.thread_ids.is_process_thread() {
            if let ProcessingState::Started(_) = self.processing_state {
                self.plugin.stop_processing();
            }
        }

        self.channel.shared_state.set_active_state(PluginActiveState::DroppedAndReadyToDeactivate);
    }
}
