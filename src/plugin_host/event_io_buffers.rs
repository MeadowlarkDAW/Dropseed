use clack_host::events::event_types::NoteEvent as ClackNoteEvent;
use clack_host::events::event_types::*;
use clack_host::events::io::EventBuffer;
use clack_host::events::spaces::CoreEventSpace;
use clack_host::events::{Event, EventHeader as ClackEventHeader, UnknownEvent};
use clack_host::utils::Cookie;
use smallvec::SmallVec;

use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ParamID;

mod sanitizer;

pub(crate) use sanitizer::PluginEventOutputSanitizer;

use crate::utils::reducing_queue::ReducFnvProducerRefMut;

use super::channel::ProcToMainParamValue;

// TODO: remove pubs
pub(crate) struct PluginEventIoBuffers {
    pub note_in_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]>,
    pub note_out_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]>,

    pub clear_note_in_buffers: SmallVec<[SharedBuffer<NoteIoEvent>; 2]>,

    pub param_event_in_buffer: Option<(SharedBuffer<ParamIoEvent>, bool)>,
    /// Only for internal plugin (e.g. timeline or macros)
    pub param_event_out_buffer: Option<SharedBuffer<ParamIoEvent>>,
}

impl PluginEventIoBuffers {
    pub fn clear_before_process(&mut self) {
        if let Some(buffer) = &mut self.param_out_buffer {
            buffer.truncate();
        }

        for buffer in self.note_out_buffers.iter().flatten() {
            buffer.truncate();
        }
    }

    pub fn write_input_events(&self, raw_event_buffer: &mut EventBuffer) -> (bool, bool) {
        let wrote_note_event = self.write_input_note_events(raw_event_buffer);
        let wrote_param_event = self.write_input_param_events(raw_event_buffer);

        (wrote_note_event, wrote_param_event)
    }

    fn write_input_note_events(&self, raw_event_buffer: &mut EventBuffer) -> bool {
        // TODO: make this clearer
        let in_events = self
            .unmixed_note_in_buffers
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i, e)))
            .flat_map(|(i, b)| b.iter().map(move |b| (i, b.borrow())));

        let mut wrote_note_event = false;

        for (note_port_index, buffer) in in_events {
            for event in buffer.iter() {
                let event = PluginIoEvent::NoteEvent {
                    note_port_index: note_port_index as i16,
                    event: *event,
                };
                event.write_to_clap_buffer(raw_event_buffer);
                wrote_note_event = true;
            }
        }

        wrote_note_event
    }

    fn write_input_param_events(&self, raw_event_buffer: &mut EventBuffer) -> bool {
        let mut wrote_param_event = false;
        for in_buf in self.unmixed_param_in_buffers.iter().flatten() {
            for event in in_buf.borrow().iter() {
                // TODO: handle cookies?
                let event = PluginIoEvent::ParamEvent { cookie: Cookie::empty(), event: *event };
                event.write_to_clap_buffer(raw_event_buffer);
                wrote_param_event = true;
            }
        }
        wrote_param_event
    }

    pub fn read_output_events(
        &mut self,
        raw_event_buffer: &EventBuffer,
        mut external_parameter_queue: Option<
            &mut ReducFnvProducerRefMut<ParamID, ProcToMainParamValue>,
        >,
        sanitizer: &mut PluginEventOutputSanitizer,
        param_target_plugin_id: u64,
    ) {
        let events_iter = raw_event_buffer
            .iter()
            .filter_map(|e| PluginIoEvent::read_from_clap(e, param_target_plugin_id));
        let events_iter = sanitizer.sanitize(events_iter);

        for event in events_iter {
            match event {
                PluginIoEvent::NoteEvent { note_port_index, event } => {
                    if let Some(Some(b)) = self.note_out_buffers.get(note_port_index as usize) {
                        b.borrow_mut().push(event)
                    }
                }
                PluginIoEvent::ParamEvent { cookie: _, event } => {
                    if let Some(buffer) = &mut self.param_out_buffer {
                        buffer.borrow_mut().push(event)
                    }

                    if let Some(queue) = external_parameter_queue.as_mut() {
                        if let Some(value) =
                            ProcToMainParamValue::from_param_event(event.event_type)
                        {
                            queue.set_or_update(ParamID::new(event.parameter_id), value);
                        }
                    }
                }
            }
        }
    }
}

// Contents of NoteBuffer
#[derive(Copy, Clone)]
pub struct NoteIoEvent {
    pub header: IoEventHeader,
    pub channel: i16,
    pub key: i16,
    pub event_type: NoteIoEventType,
}

// Contents of ParamBuffer
#[derive(Copy, Clone)]
pub struct ParamIoEvent {
    pub header: IoEventHeader,
    pub parameter_id: u32,
    pub event_type: ParamIoEventType,
    pub plugin_instance_id: u64,
}

// Contains common data
#[derive(Copy, Clone)]
pub struct IoEventHeader {
    pub time: u32,
    // TODO: add event flags here when we implement them
}

#[derive(Copy, Clone)]
pub enum NoteIoEventType {
    On { velocity: f64 },
    Expression { expression_type: NoteExpressionType, value: f64 },
    Choke,
    Off { velocity: f64 },
}

#[derive(Copy, Clone)]
pub enum ParamIoEventType {
    Value(f64),
    Modulation(f64),
    BeginGesture,
    EndGesture,
}

#[derive(Copy, Clone)]
pub enum PluginIoEvent {
    NoteEvent { note_port_index: i16, event: NoteIoEvent },
    ParamEvent { cookie: Cookie, event: ParamIoEvent },
}

impl PluginIoEvent {
    pub fn read_from_clap(
        clap_event: &UnknownEvent,
        target_plugin_instance_id: u64,
    ) -> Option<Self> {
        match clap_event.as_core_event()? {
            CoreEventSpace::NoteOn(NoteOnEvent(e)) => Some(PluginIoEvent::NoteEvent {
                note_port_index: e.port_index(),
                event: NoteIoEvent {
                    channel: e.channel(),
                    key: e.key(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: NoteIoEventType::On { velocity: e.velocity() },
                },
            }),
            CoreEventSpace::NoteOff(NoteOffEvent(e)) => Some(PluginIoEvent::NoteEvent {
                note_port_index: e.port_index(),
                event: NoteIoEvent {
                    channel: e.channel(),
                    key: e.key(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: NoteIoEventType::Off { velocity: e.velocity() },
                },
            }),
            CoreEventSpace::NoteChoke(NoteChokeEvent(e)) => Some(PluginIoEvent::NoteEvent {
                note_port_index: e.port_index(),
                event: NoteIoEvent {
                    channel: e.channel(),
                    key: e.key(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: NoteIoEventType::Choke,
                },
            }),
            CoreEventSpace::NoteExpression(e) => Some(PluginIoEvent::NoteEvent {
                note_port_index: e.port_index(),
                event: NoteIoEvent {
                    channel: e.channel(),
                    key: e.key(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: NoteIoEventType::Expression {
                        expression_type: e.expression_type()?,
                        value: e.value(),
                    },
                },
            }),

            CoreEventSpace::ParamValue(e) => Some(PluginIoEvent::ParamEvent {
                cookie: e.cookie(),
                event: ParamIoEvent {
                    plugin_instance_id: target_plugin_instance_id,
                    parameter_id: e.param_id(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: ParamIoEventType::Value(e.value()),
                },
            }),
            CoreEventSpace::ParamMod(e) => Some(PluginIoEvent::ParamEvent {
                cookie: e.cookie(),
                event: ParamIoEvent {
                    plugin_instance_id: target_plugin_instance_id,
                    parameter_id: e.param_id(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: ParamIoEventType::Modulation(e.value()),
                },
            }),
            CoreEventSpace::ParamGestureBegin(e) => Some(PluginIoEvent::ParamEvent {
                cookie: Cookie::empty(),
                event: ParamIoEvent {
                    plugin_instance_id: target_plugin_instance_id,
                    parameter_id: e.param_id(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: ParamIoEventType::BeginGesture,
                },
            }),
            CoreEventSpace::ParamGestureEnd(e) => Some(PluginIoEvent::ParamEvent {
                cookie: Cookie::empty(),
                event: ParamIoEvent {
                    plugin_instance_id: target_plugin_instance_id,
                    parameter_id: e.param_id(),
                    header: IoEventHeader { time: e.header().time() },
                    event_type: ParamIoEventType::EndGesture,
                },
            }),

            // TODO: handle MIDI events & note end events
            _ => None,
        }
    }

    pub fn write_to_clap_buffer(&self, buffer: &mut EventBuffer) {
        // TODO: Clack event types are a mouthful
        match self {
            PluginIoEvent::NoteEvent {
                note_port_index,
                event: NoteIoEvent { event_type, key, channel, header: IoEventHeader { time } },
            } => match event_type {
                NoteIoEventType::On { velocity } => buffer.push(
                    NoteOnEvent(ClackNoteEvent::new(
                        ClackEventHeader::new(*time),
                        -1,
                        *note_port_index,
                        *key,
                        *channel,
                        *velocity,
                    ))
                    .as_unknown(),
                ),
                NoteIoEventType::Expression { expression_type, value } => buffer.push(
                    NoteExpressionEvent::new(
                        ClackEventHeader::new(*time),
                        -1,
                        *note_port_index,
                        *key,
                        *channel,
                        *value,
                        *expression_type,
                    )
                    .as_unknown(),
                ),

                NoteIoEventType::Choke => buffer.push(
                    NoteChokeEvent(ClackNoteEvent::new(
                        ClackEventHeader::new(*time),
                        -1,
                        *note_port_index,
                        *key,
                        *channel,
                        0.0,
                    ))
                    .as_unknown(),
                ),

                NoteIoEventType::Off { velocity } => buffer.push(
                    NoteOffEvent(ClackNoteEvent::new(
                        ClackEventHeader::new(*time),
                        -1,
                        *note_port_index,
                        *key,
                        *channel,
                        *velocity,
                    ))
                    .as_unknown(),
                ),
            },
            PluginIoEvent::ParamEvent {
                cookie,
                event:
                    ParamIoEvent {
                        header: IoEventHeader { time },
                        parameter_id,
                        event_type,
                        plugin_instance_id: _,
                    },
            } => match event_type {
                ParamIoEventType::Value(value) => buffer.push(
                    ParamValueEvent::new(
                        ClackEventHeader::new(*time),
                        *cookie,
                        -1,
                        *parameter_id,
                        -1,
                        -1,
                        -1,
                        *value,
                    )
                    .as_unknown(),
                ),
                ParamIoEventType::Modulation(modulation_amount) => buffer.push(
                    ParamModEvent::new(
                        ClackEventHeader::new(*time),
                        *cookie,
                        -1,
                        *parameter_id,
                        -1,
                        -1,
                        -1,
                        *modulation_amount,
                    )
                    .as_unknown(),
                ),
                ParamIoEventType::BeginGesture => buffer.push(
                    ParamGestureBeginEvent::new(ClackEventHeader::new(*time), *parameter_id)
                        .as_unknown(),
                ),
                ParamIoEventType::EndGesture => buffer.push(
                    ParamGestureEndEvent::new(ClackEventHeader::new(*time), *parameter_id)
                        .as_unknown(),
                ),
            },
        }
    }
}
