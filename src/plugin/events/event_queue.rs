use clap_sys::events::clap_event_header as ClapEventHeader;
use std::iter::Iterator;

use super::{
    EventMidi, EventMidi2, EventMidiSysex, EventNote, EventNoteExpression, EventParamGesture,
    EventParamMod, EventParamValue, EventTransport,
};

// TODO: Use an event queue that supports variable sizes for messages to
// save on memory. The majority of events will be about half the size or
// less than the less common maximum-sized event `EventTransport`.

pub struct EventQueue {
    pub(crate) events: Vec<ProcEvent>,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self { events: Vec::with_capacity(capacity) }
    }

    #[inline]
    pub fn push(&mut self, event: ProcEvent) {
        if self.events.len() >= self.events.capacity() {
            log::warn!("Event queue has exceeded its capacity. This will cause an allocation on the audio thread.");
        }

        self.events.push(event);
    }

    pub fn pop(&mut self) -> Option<ProcEvent> {
        self.events.pop()
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a ProcEvent> {
        let s = self.events.iter();

        self.events.iter()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

#[derive(Clone, Copy)]
pub enum ProcEventRef<'a> {
    Note(&'a EventNote),
    NoteExpression(&'a EventNoteExpression),
    ParamValue(&'a EventParamValue),
    ParamMod(&'a EventParamMod),
    ParamGesture(&'a EventParamGesture),
    Transport(&'a EventTransport),
    Midi(&'a EventMidi),
    MidiSysex(&'a EventMidiSysex),
    Midi2(&'a EventMidi2),
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union ProcEvent {
    note: EventNote,
    note_expression: EventNoteExpression,
    param_value: EventParamValue,
    param_mod: EventParamMod,
    param_gesture: EventParamGesture,
    transport: EventTransport,
    midi: EventMidi,
    midi_sysex: EventMidiSysex,
    midi2: EventMidi2,
}

impl ProcEvent {
    pub fn raw_pointer(&self) -> *const ClapEventHeader {
        unsafe { &self.note.0.header }
    }

    pub fn from_ref<'a>(event: ProcEventRef<'a>) -> Self {
        match event {
            ProcEventRef::Note(e) => ProcEvent { note: *e },
            ProcEventRef::NoteExpression(e) => ProcEvent { note_expression: *e },
            ProcEventRef::ParamValue(e) => ProcEvent { param_value: *e },
            ProcEventRef::ParamMod(e) => ProcEvent { param_mod: *e },
            ProcEventRef::ParamGesture(e) => ProcEvent { param_gesture: *e },
            ProcEventRef::Transport(e) => ProcEvent { transport: *e },
            ProcEventRef::Midi(e) => ProcEvent { midi: *e },
            ProcEventRef::MidiSysex(e) => ProcEvent { midi_sysex: *e },
            ProcEventRef::Midi2(e) => ProcEvent { midi2: *e },
        }
    }

    pub fn get<'a>(&'a self) -> Result<ProcEventRef<'a>, ()> {
        // The event header is always the first bytes in every event.
        let header = unsafe { self.note.0.header };

        match header.type_ {
            clap_sys::events::CLAP_EVENT_NOTE_ON
            | clap_sys::events::CLAP_EVENT_NOTE_OFF
            | clap_sys::events::CLAP_EVENT_NOTE_CHOKE
            | clap_sys::events::CLAP_EVENT_NOTE_END => {
                Ok(ProcEventRef::Note(unsafe { &self.note }))
            }
            clap_sys::events::CLAP_EVENT_NOTE_EXPRESSION => {
                Ok(ProcEventRef::NoteExpression(unsafe { &self.note_expression }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_VALUE => {
                Ok(ProcEventRef::ParamValue(unsafe { &self.param_value }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_MOD => {
                Ok(ProcEventRef::ParamMod(unsafe { &self.param_mod }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_GESTURE_BEGIN
            | clap_sys::events::CLAP_EVENT_PARAM_GESTURE_END => {
                Ok(ProcEventRef::ParamGesture(unsafe { &self.param_gesture }))
            }
            clap_sys::events::CLAP_EVENT_TRANSPORT => {
                Ok(ProcEventRef::Transport(unsafe { &self.transport }))
            }
            clap_sys::events::CLAP_EVENT_MIDI => Ok(ProcEventRef::Midi(unsafe { &self.midi })),
            clap_sys::events::CLAP_EVENT_MIDI_SYSEX => {
                Ok(ProcEventRef::MidiSysex(unsafe { &self.midi_sysex }))
            }
            clap_sys::events::CLAP_EVENT_MIDI2 => Ok(ProcEventRef::Midi2(unsafe { &self.midi2 })),
            _ => Err(()),
        }
    }
}

impl From<EventNote> for ProcEvent {
    fn from(e: EventNote) -> Self {
        ProcEvent { note: e }
    }
}

impl From<EventNoteExpression> for ProcEvent {
    fn from(e: EventNoteExpression) -> Self {
        ProcEvent { note_expression: e }
    }
}

impl From<EventParamValue> for ProcEvent {
    fn from(e: EventParamValue) -> Self {
        ProcEvent { param_value: e }
    }
}

impl From<EventParamMod> for ProcEvent {
    fn from(e: EventParamMod) -> Self {
        ProcEvent { param_mod: e }
    }
}

impl From<EventParamGesture> for ProcEvent {
    fn from(e: EventParamGesture) -> Self {
        ProcEvent { param_gesture: e }
    }
}

impl From<EventTransport> for ProcEvent {
    fn from(e: EventTransport) -> Self {
        ProcEvent { transport: e }
    }
}

impl From<EventMidi> for ProcEvent {
    fn from(e: EventMidi) -> Self {
        ProcEvent { midi: e }
    }
}

impl From<EventMidiSysex> for ProcEvent {
    fn from(e: EventMidiSysex) -> Self {
        ProcEvent { midi_sysex: e }
    }
}

impl From<EventMidi2> for ProcEvent {
    fn from(e: EventMidi2) -> Self {
        ProcEvent { midi2: e }
    }
}
