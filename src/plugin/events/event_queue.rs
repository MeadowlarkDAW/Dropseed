use clap_sys::events::clap_event_header as ClapEventHeader;

use super::{
    EventMidi, EventMidi2, EventMidiSysex, EventNote, EventNoteExpression, EventParamGesture,
    EventParamMod, EventParamValue, EventTransport,
};

// TODO: Use an event queue that supports variable sizes for messages to
// save on memory. The majority of events will be about half the size or
// less than the less common maximum-sized event `EventTransport`.

pub struct EventQueue {
    pub(crate) events: Vec<PluginEvent>,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self { events: Vec::with_capacity(capacity) }
    }

    #[inline]
    pub fn push(&mut self, event: PluginEvent) {
        if self.events.len() >= self.events.capacity() {
            log::warn!("Event queue has exceeded its capacity. This will cause an allocation on the audio thread.");
        }

        self.events.push(event);
    }

    pub fn pop(&mut self) -> Option<PluginEvent> {
        self.events.pop()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

#[derive(Clone, Copy)]
pub enum PluginEventRef<'a> {
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
pub union PluginEvent {
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

impl PluginEvent {
    pub fn raw_pointer(&self) -> *const ClapEventHeader {
        unsafe { &self.note.0.header }
    }

    pub fn from_ref<'a>(event: PluginEventRef<'a>) -> Self {
        match event {
            PluginEventRef::Note(e) => PluginEvent { note: *e },
            PluginEventRef::NoteExpression(e) => PluginEvent { note_expression: *e },
            PluginEventRef::ParamValue(e) => PluginEvent { param_value: *e },
            PluginEventRef::ParamMod(e) => PluginEvent { param_mod: *e },
            PluginEventRef::ParamGesture(e) => PluginEvent { param_gesture: *e },
            PluginEventRef::Transport(e) => PluginEvent { transport: *e },
            PluginEventRef::Midi(e) => PluginEvent { midi: *e },
            PluginEventRef::MidiSysex(e) => PluginEvent { midi_sysex: *e },
            PluginEventRef::Midi2(e) => PluginEvent { midi2: *e },
        }
    }

    pub fn get<'a>(&'a self) -> Result<PluginEventRef<'a>, ()> {
        // The event header is always the first bytes in every event.
        let header = unsafe { self.note.0.header };

        match header.type_ {
            clap_sys::events::CLAP_EVENT_NOTE_ON
            | clap_sys::events::CLAP_EVENT_NOTE_OFF
            | clap_sys::events::CLAP_EVENT_NOTE_CHOKE
            | clap_sys::events::CLAP_EVENT_NOTE_END => {
                Ok(PluginEventRef::Note(unsafe { &self.note }))
            }
            clap_sys::events::CLAP_EVENT_NOTE_EXPRESSION => {
                Ok(PluginEventRef::NoteExpression(unsafe { &self.note_expression }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_VALUE => {
                Ok(PluginEventRef::ParamValue(unsafe { &self.param_value }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_MOD => {
                Ok(PluginEventRef::ParamMod(unsafe { &self.param_mod }))
            }
            clap_sys::events::CLAP_EVENT_PARAM_GESTURE_BEGIN
            | clap_sys::events::CLAP_EVENT_PARAM_GESTURE_END => {
                Ok(PluginEventRef::ParamGesture(unsafe { &self.param_gesture }))
            }
            clap_sys::events::CLAP_EVENT_TRANSPORT => {
                Ok(PluginEventRef::Transport(unsafe { &self.transport }))
            }
            clap_sys::events::CLAP_EVENT_MIDI => Ok(PluginEventRef::Midi(unsafe { &self.midi })),
            clap_sys::events::CLAP_EVENT_MIDI_SYSEX => {
                Ok(PluginEventRef::MidiSysex(unsafe { &self.midi_sysex }))
            }
            clap_sys::events::CLAP_EVENT_MIDI2 => Ok(PluginEventRef::Midi2(unsafe { &self.midi2 })),
            _ => Err(()),
        }
    }
}

impl From<EventNote> for PluginEvent {
    fn from(e: EventNote) -> Self {
        PluginEvent { note: e }
    }
}

impl From<EventNoteExpression> for PluginEvent {
    fn from(e: EventNoteExpression) -> Self {
        PluginEvent { note_expression: e }
    }
}

impl From<EventParamValue> for PluginEvent {
    fn from(e: EventParamValue) -> Self {
        PluginEvent { param_value: e }
    }
}

impl From<EventParamMod> for PluginEvent {
    fn from(e: EventParamMod) -> Self {
        PluginEvent { param_mod: e }
    }
}

impl From<EventParamGesture> for PluginEvent {
    fn from(e: EventParamGesture) -> Self {
        PluginEvent { param_gesture: e }
    }
}

impl From<EventTransport> for PluginEvent {
    fn from(e: EventTransport) -> Self {
        PluginEvent { transport: e }
    }
}

impl From<EventMidi> for PluginEvent {
    fn from(e: EventMidi) -> Self {
        PluginEvent { midi: e }
    }
}

impl From<EventMidiSysex> for PluginEvent {
    fn from(e: EventMidiSysex) -> Self {
        PluginEvent { midi_sysex: e }
    }
}

impl From<EventMidi2> for PluginEvent {
    fn from(e: EventMidi2) -> Self {
        PluginEvent { midi2: e }
    }
}
