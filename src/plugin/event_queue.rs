use bytemuck::{bytes_of, try_from_bytes};
use crossbeam::channel;
use fnv::FnvHashMap;
use rtrb_basedrop::{Consumer, Producer};
use rusty_daw_core::atomic::AtomicF64;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use std::mem::MaybeUninit;

use super::events::{
    EventHeader, EventMidi, EventMidi2, EventMidiSysex, EventNote, EventNoteExpression,
    EventParamGesture, EventParamMod, EventParamValue, EventTransport, EventType, PluginEvent,
};

// TODO: Use an event queue that supports variable sizes for messages to
// save on memory. The majority of events will be about half the size or
// less than the less common maximum-sized event `EventTransport`.

pub struct EventQueueAudioThread {}

pub struct EventQueueMainThread {}

pub struct AllocatedEvent {
    data: [u8; std::mem::size_of::<EventTransport>()],
}

impl AllocatedEvent {
    pub fn from_event<'a>(event: PluginEvent<'a>) -> Self {
        let raw_bytes = match event {
            PluginEvent::Note(e) => bytes_of(e),
            PluginEvent::NoteExpression(e) => bytes_of(e),
            PluginEvent::ParamValue(e) => bytes_of(e),
            PluginEvent::ParamMod(e) => bytes_of(e),
            PluginEvent::ParamGesture(e) => bytes_of(e),
            PluginEvent::Transport(e) => bytes_of(e),
            PluginEvent::Midi(e) => bytes_of(e),
            PluginEvent::MidiSysex(e) => bytes_of(e),
            PluginEvent::Midi2(e) => bytes_of(e),
        };

        debug_assert!(raw_bytes.len() <= std::mem::size_of::<EventTransport>());

        // This is safe because we ensure that only the correct number of bytes
        // will be read via the event.header.size value, which the constructor
        // of each event ensures is correct.
        let mut data: [u8; std::mem::size_of::<EventTransport>()] =
            unsafe { MaybeUninit::uninit().assume_init() };

        data[0..raw_bytes.len()].copy_from_slice(raw_bytes);

        Self { data }
    }

    pub fn get<'a>(&'a self) -> Result<PluginEvent<'a>, ()> {
        // The event header is always the first bytes in every event.
        let header: &EventHeader =
            match try_from_bytes(&self.data[0..std::mem::size_of::<EventHeader>()]) {
                Ok(header) => header,
                Err(_) => {
                    return Err(());
                }
            };

        let event_type = if let Some(event_type) = header.event_type() {
            event_type
        } else {
            return Err(());
        };

        match event_type {
            EventType::NoteOn | EventType::NoteOff | EventType::NoteChoke | EventType::NoteEnd => {
                let event: &EventNote =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventNote>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::Note(event))
            }
            EventType::NoteExpression => {
                let event: &EventNoteExpression =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventNoteExpression>()])
                    {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::NoteExpression(event))
            }
            EventType::ParamValue => {
                let event: &EventParamValue =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventParamValue>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::ParamValue(event))
            }
            EventType::ParamMod => {
                let event: &EventParamMod =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventParamMod>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::ParamMod(event))
            }
            EventType::ParamGestureBegin | EventType::ParamGestureEnd => {
                let event: &EventParamGesture =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventParamGesture>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::ParamGesture(event))
            }
            EventType::Transport => {
                let event: &EventTransport =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventTransport>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::Transport(event))
            }
            EventType::Midi => {
                let event: &EventMidi =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventMidi>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::Midi(event))
            }
            EventType::MidiSysex => {
                let event: &EventMidiSysex =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventMidiSysex>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::MidiSysex(event))
            }
            EventType::Midi2 => {
                let event: &EventMidi2 =
                    match try_from_bytes(&self.data[0..std::mem::size_of::<EventMidi2>()]) {
                        Ok(e) => e,
                        Err(_) => return Err(()),
                    };
                Ok(PluginEvent::Midi2(event))
            }
        }
    }
}
