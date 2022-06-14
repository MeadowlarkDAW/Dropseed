//! This is a direct 1:1 port of the events structure as specified in the CLAP plugin spec.
//! <https://github.com/free-audio/clap/blob/main/include/clap/events.h>
//!
//! I chose to use the raw C format rather than a "rusty" version of it so we won't have to
//! do any (potentially expensive) type conversions on the fly going to/from CLAP plugins.
//!
//! Note I have chosen to "rust-ify" other non-performance critical aspects of
//! the spec such as plugin description and audio port configuration.

use bitflags::bitflags;
use std::hash::Hash;

use clap_sys::events::clap_event_flags as ClapEventFlags;
use clap_sys::events::clap_event_header as ClapEventHeader;
use clap_sys::events::clap_event_midi as ClapEventMidi;
use clap_sys::events::clap_event_midi2 as ClapEventMidi2;
use clap_sys::events::clap_event_midi_sysex as ClapEventMidiSysex;
use clap_sys::events::clap_event_note as ClapEventNote;
use clap_sys::events::clap_event_note_expression as ClapEventNoteExpression;
use clap_sys::events::clap_event_param_gesture as ClapEventParamGesture;
use clap_sys::events::clap_event_param_mod as ClapEventParamMod;
use clap_sys::events::clap_event_param_value as ClapEventParamValue;
use clap_sys::events::clap_event_transport as ClapEventTransport;
use clap_sys::events::clap_transport_flags as ClapTransportFlags;

use crate::{FixedPoint64, ParamID};

pub mod event_queue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventBeatTime(pub(crate) FixedPoint64);

impl EventBeatTime {
    #[inline]
    pub fn from_f64(val: f64) -> Self {
        Self(FixedPoint64::from_f64(val))
    }

    #[inline]
    pub fn as_f64(&self) -> f64 {
        self.0.as_f64()
    }
}

impl From<f64> for EventBeatTime {
    fn from(v: f64) -> Self {
        Self::from_f64(v)
    }
}

impl From<EventBeatTime> for f64 {
    fn from(v: EventBeatTime) -> Self {
        v.as_f64()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventSecTime(pub(crate) FixedPoint64);

impl EventSecTime {
    #[inline]
    pub fn from_f64(val: f64) -> Self {
        Self(FixedPoint64::from_f64(val))
    }

    #[inline]
    pub fn as_f64(&self) -> f64 {
        self.0.as_f64()
    }
}

impl From<f64> for EventSecTime {
    fn from(v: f64) -> Self {
        Self::from_f64(v)
    }
}

impl From<EventSecTime> for f64 {
    fn from(v: EventSecTime) -> Self {
        v.as_f64()
    }
}

bitflags! {
    pub struct EventFlags: ClapEventFlags {
        // Indicate a live user event, for example a user turning a physical knob
        // or playing a physical key.
        const EVENT_IS_LIVE = clap_sys::events::CLAP_EVENT_IS_LIVE;

        // Indicate that the event should not be recorded.
        // For example this is useful when a parameter changes because of a MIDI CC,
        // because if the host records both the MIDI CC automation and the parameter
        // automation there will be a conflict.
        const EVENT_DONT_RECORD = clap_sys::events::CLAP_EVENT_DONT_RECORD;
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventNoteType {
    NoteOn = clap_sys::events::CLAP_EVENT_NOTE_ON,
    NoteOff = clap_sys::events::CLAP_EVENT_NOTE_OFF,
    NoteChoke = clap_sys::events::CLAP_EVENT_NOTE_CHOKE,
    NoteEnd = clap_sys::events::CLAP_EVENT_NOTE_END,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventNote(ClapEventNote);

impl EventNote {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `event_type`: see `EventNoteType`
    /// - `note_id`:  -1 if unspecified, otherwise >=0
    /// - `port_index`:  The index of the port
    /// - `channel`: `[0..15]`
    /// - `key`: `[0..127]`
    /// - `velocity`: `[0.0, 1.0]`
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        event_type: EventNoteType,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        velocity: f64,
    ) -> Self {
        Self(ClapEventNote {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventNote>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: unsafe { *(&event_type as *const EventNoteType as *const u16) },
                flags: event_flags.bits(),
            },
            note_id,
            port_index,
            channel,
            key,
            velocity,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventNote) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventNote>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn event_type(&self) -> EventNoteType {
        // Safe because this enum is represented with u16, and the constructor
        // and the event queue ensures that this is a valid value.
        unsafe { *(&self.0.header.type_ as *const u16 as *const EventNoteType) }
    }

    /// -1 if unspecified, otherwise >=0
    pub fn note_id(&self) -> i32 {
        self.0.note_id
    }

    pub fn port_index(&self) -> i16 {
        self.0.port_index
    }

    /// `[0..15]`
    pub fn channel(&self) -> i16 {
        self.0.channel
    }

    /// `[0..127]`
    pub fn key(&self) -> i16 {
        self.0.key
    }

    /// `[0.0, 1.0]`
    pub fn velocity(&self) -> f64 {
        self.0.velocity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExpressionType {
    Known(NoteExpression),
    Unkown(i32),
}

impl NoteExpressionType {
    #[inline]
    pub fn from_i32(val: i32) -> Self {
        if val >= 0 && val <= clap_sys::events::CLAP_NOTE_EXPRESSION_PRESSURE {
            // Safe because this enum is represented with an i32, and we checked that the
            // value is within range.
            NoteExpressionType::Known(unsafe { *(&val as *const i32 as *const NoteExpression) })
        } else {
            NoteExpressionType::Unkown(val)
        }
    }

    #[inline]
    pub fn to_i32(&self) -> i32 {
        match self {
            NoteExpressionType::Known(val) => {
                // Safe because this enum is represented with an i32.
                unsafe { *(val as *const NoteExpression as *const i32) }
            }
            NoteExpressionType::Unkown(val) => *val,
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExpression {
    /// `[0.0, 4.0]`, plain = 20.0 * log(x)
    Volume = clap_sys::events::CLAP_NOTE_EXPRESSION_VOLUME,
    /// pan, 0.0 left, 0.5 center, 1.0 right
    Pan = clap_sys::events::CLAP_NOTE_EXPRESSION_PAN,
    /// relative tuning in semitone, from -120.0 to +120.0
    Tuning = clap_sys::events::CLAP_NOTE_EXPRESSION_TUNING,
    /// `[0.0, 1.0]`
    Vibrato = clap_sys::events::CLAP_NOTE_EXPRESSION_VIBRATO,
    /// `[0.0, 1.0]`
    Expression = clap_sys::events::CLAP_NOTE_EXPRESSION_EXPRESSION,
    /// `[0.0, 1.0]`
    Brightness = clap_sys::events::CLAP_NOTE_EXPRESSION_BRIGHTNESS,
    /// `[0.0, 1.0]`
    Pressure = clap_sys::events::CLAP_NOTE_EXPRESSION_PRESSURE,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventNoteExpression(ClapEventNoteExpression);

impl EventNoteExpression {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `expression_id`: see `NoteExpressionType`
    /// - `note_id`:  -1 if unspecified, otherwise >=0
    /// - `port_index`:  The index of the port
    /// - `channel`: `[0..15]`
    /// - `key`: `[0..127]`
    /// - `value`: see `NoteExpression` for range
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        expression_id: NoteExpressionType,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        value: f64,
    ) -> Self {
        Self(ClapEventNoteExpression {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventNoteExpression>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_NOTE_EXPRESSION,
                flags: event_flags.bits(),
            },
            expression_id: expression_id.to_i32(),
            note_id,
            port_index,
            channel,
            key,
            value,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventNoteExpression) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventNoteExpression>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn expression_id(&self) -> NoteExpressionType {
        NoteExpressionType::from_i32(self.0.expression_id)
    }

    /// -1 if unspecified, otherwise >=0
    pub fn note_id(&self) -> i32 {
        self.0.note_id
    }

    pub fn port_index(&self) -> i16 {
        self.0.port_index
    }

    /// `[0..15]`
    pub fn channel(&self) -> i16 {
        self.0.channel
    }

    /// `[0..127]`
    pub fn key(&self) -> i16 {
        self.0.key
    }

    pub fn value(&self) -> f64 {
        self.0.value
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventParamValue(ClapEventParamValue);

unsafe impl Send for EventParamValue {}
unsafe impl Sync for EventParamValue {}

impl EventParamValue {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `param_id`: the ID of the parameter
    /// - `note_id`:  -1 if unspecified, otherwise >=0
    /// - `port_index`:  The index of the port
    /// - `channel`: `[0..15]`
    /// - `key`: `[0..127]`
    /// - `value`: the plain value of the parameter
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        param_id: ParamID,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        value: f64,
    ) -> Self {
        Self(ClapEventParamValue {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventParamValue>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_PARAM_VALUE,
                flags: event_flags.bits(),
            },
            param_id: param_id.0,
            cookie: std::ptr::null_mut(),
            note_id,
            port_index,
            channel,
            key,
            value,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventParamValue) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventParamValue>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn param_id(&self) -> ParamID {
        ParamID(self.0.param_id)
    }

    /// -1 if unspecified, otherwise >=0
    pub fn note_id(&self) -> i32 {
        self.0.note_id
    }

    pub fn port_index(&self) -> i16 {
        self.0.port_index
    }

    /// `[0..15]`
    pub fn channel(&self) -> i16 {
        self.0.channel
    }

    /// `[0..127]`
    pub fn key(&self) -> i16 {
        self.0.key
    }

    /// The plain value of the parameter.
    pub fn value(&self) -> f64 {
        self.0.value
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventParamMod(ClapEventParamMod);

unsafe impl Send for EventParamMod {}
unsafe impl Sync for EventParamMod {}

impl EventParamMod {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `param_id`: the ID of the parameter
    /// - `note_id`:  -1 if unspecified, otherwise >=0
    /// - `port_index`:  The index of the port
    /// - `channel`: `[0..15]`
    /// - `key`: `[0..127]`
    /// - `amount`: modulation amount
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        param_id: ParamID,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        amount: f64,
    ) -> Self {
        Self(ClapEventParamMod {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventParamMod>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_PARAM_MOD,
                flags: event_flags.bits(),
            },
            param_id: param_id.0,
            cooke: std::ptr::null_mut(),
            note_id,
            port_index,
            channel,
            key,
            amount,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventParamMod) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventParamMod>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn param_id(&self) -> ParamID {
        ParamID(self.0.param_id)
    }

    /// -1 if unspecified, otherwise >=0
    pub fn note_id(&self) -> i32 {
        self.0.note_id
    }

    pub fn port_index(&self) -> i16 {
        self.0.port_index
    }

    /// `[0..15]`
    pub fn channel(&self) -> i16 {
        self.0.channel
    }

    /// `[0..127]`
    pub fn key(&self) -> i16 {
        self.0.key
    }

    /// Modulation amount.
    pub fn amount(&self) -> f64 {
        self.0.amount
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamGestureType {
    GestureBegin = clap_sys::events::CLAP_EVENT_PARAM_GESTURE_BEGIN,
    GestureEnd = clap_sys::events::CLAP_EVENT_PARAM_GESTURE_END,
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventParamGesture(ClapEventParamGesture);

impl EventParamGesture {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `gesture`: the type of gesture
    /// - `param_id`: the ID of the parameter
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        gesture_type: ParamGestureType,
        param_id: ParamID,
    ) -> Self {
        Self(ClapEventParamGesture {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventParamGesture>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: unsafe { *(&gesture_type as *const ParamGestureType as *const u16) },
                flags: event_flags.bits(),
            },
            param_id: param_id.0,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventParamGesture) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventParamGesture>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn gesture_type(&self) -> ParamGestureType {
        // Safe because this enum is represented with u16, and the constructor
        // and the event queue ensures that this is a valid value.
        unsafe { *(&self.0.header.type_ as *const u16 as *const ParamGestureType) }
    }

    pub fn is_begin(&self) -> bool {
        self.gesture_type() == ParamGestureType::GestureBegin
    }

    pub fn param_id(&self) -> ParamID {
        ParamID(self.0.param_id)
    }
}

bitflags! {
    pub struct TransportFlags: ClapTransportFlags {
        const HAS_TEMPO = clap_sys::events::CLAP_TRANSPORT_HAS_TEMPO;
        const HAS_BEATS_TIMELINE = clap_sys::events::CLAP_TRANSPORT_HAS_BEATS_TIMELINE;
        const HAS_SECONDS_TIMELINE = clap_sys::events::CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
        const HAS_TIME_SIGNATURE = clap_sys::events::CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
        const IS_PLAYING = clap_sys::events::CLAP_TRANSPORT_IS_PLAYING;
        const IS_RECORDING = clap_sys::events::CLAP_TRANSPORT_IS_RECORDING;
        const IS_LOOP_ACTIVE = clap_sys::events::CLAP_TRANSPORT_IS_LOOP_ACTIVE;
        const IS_WITHIN_PRE_ROLL = clap_sys::events::CLAP_TRANSPORT_IS_WITHIN_PRE_ROLL;
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventTransport(ClapEventTransport);

impl EventTransport {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `transport_flags`: see `TransportFlags`
    /// - `song_pos_beats`: position in beats
    /// - `song_pos_seconds`: position in seconds
    /// - `tempo`: in bpm
    /// - `tempo_inc`: tempo increment for each sample until the next time info event
    /// - `loop_start_beats`: position of the loop start point in beats
    /// - `loop_end_beats`: position of the loop end point in beats
    /// - `loop_start_seconds`: position of the loop start point in seconds
    /// - `loop_end_seconds`: position of the loop end point in seconds
    /// - `bar_start`: start position of the current bar in beats
    /// - `bar_number`: bar at song pos 0 has the number 0
    /// - `tsig_num`: time signature numerator
    /// - `tsig_denom`: time signature denominator
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        transport_flags: TransportFlags,
        song_pos_beats: EventBeatTime,
        song_pos_seconds: EventSecTime,
        tempo: f64,
        tempo_inc: f64,
        loop_start_beats: EventBeatTime,
        loop_end_beats: EventBeatTime,
        loop_start_seconds: EventSecTime,
        loop_end_seconds: EventSecTime,
        bar_start: EventBeatTime,
        bar_number: i32,
        tsig_num: u16,
        tsig_denom: u16,
    ) -> Self {
        Self(ClapEventTransport {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventTransport>() as u32,
                time,
                space_id,
                type_: clap_sys::events::CLAP_EVENT_TRANSPORT,
                flags: event_flags.bits(),
            },
            flags: transport_flags.bits(),
            song_pos_beats: song_pos_beats.0 .0,
            song_pos_seconds: song_pos_seconds.0 .0,
            tempo,
            tempo_inc,
            loop_start_beats: loop_start_beats.0 .0,
            loop_end_beats: loop_end_beats.0 .0,
            loop_start_seconds: loop_start_seconds.0 .0,
            loop_end_seconds: loop_end_seconds.0 .0,
            bar_start: bar_start.0 .0,
            bar_number,
            tsig_num,
            tsig_denom,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventTransport) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventTransport>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    pub fn transport_flags(&self) -> TransportFlags {
        TransportFlags::from_bits_truncate(self.0.flags)
    }

    /// position in beats
    pub fn song_pos_beats(&self) -> EventBeatTime {
        EventBeatTime(FixedPoint64(self.0.song_pos_beats))
    }

    /// position in seconds
    pub fn song_pos_seconds(&self) -> EventSecTime {
        EventSecTime(FixedPoint64(self.0.song_pos_seconds))
    }

    /// in bpm
    pub fn tempo(&self) -> f64 {
        self.0.tempo
    }

    /// tempo increment for each sample until the next time info event
    pub fn tempo_inc(&self) -> f64 {
        self.0.tempo_inc
    }

    /// position of the loop start point in beats
    pub fn loop_start_beats(&self) -> EventBeatTime {
        EventBeatTime(FixedPoint64(self.0.loop_start_beats))
    }

    /// position of the loop end point in beats
    pub fn loop_end_beats(&self) -> EventBeatTime {
        EventBeatTime(FixedPoint64(self.0.loop_end_beats))
    }

    /// position of the loop start point in seconds
    pub fn loop_start_seconds(&self) -> EventSecTime {
        EventSecTime(FixedPoint64(self.0.loop_start_seconds))
    }

    /// position of the loop end point in seconds
    pub fn loop_end_seconds(&self) -> EventSecTime {
        EventSecTime(FixedPoint64(self.0.loop_end_seconds))
    }

    /// start position of the current bar in beats
    pub fn bar_start(&self) -> EventBeatTime {
        EventBeatTime(FixedPoint64(self.0.bar_start))
    }

    /// bar at song pos 0 has the number 0
    pub fn bar_number(&self) -> i32 {
        self.0.bar_number
    }

    /// time signature numerator
    pub fn tsig_num(&self) -> u16 {
        self.0.tsig_num
    }

    /// time signature denominator
    pub fn tsig_denom(&self) -> u16 {
        self.0.tsig_denom
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventMidi(ClapEventMidi);

impl EventMidi {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `port_index`: the index of the MIDI port
    /// - `data`: the raw MIDI data
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        data: [u8; 3],
    ) -> Self {
        Self(ClapEventMidi {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventMidi>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_MIDI,
                flags: event_flags.bits(),
            },
            port_index,
            data,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventMidi) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventMidi>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    /// The index of the MIDI port.
    pub fn port_index(&self) -> u16 {
        self.0.port_index
    }

    /// The raw MIDI data.
    pub fn data(&self) -> [u8; 3] {
        self.0.data
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventMidiSysex(ClapEventMidiSysex);

unsafe impl Send for EventMidiSysex {}
unsafe impl Sync for EventMidiSysex {}

impl EventMidiSysex {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `port_index`: the index of the MIDI port
    /// - `buffer_pointer`: the pointer to the raw data buffer
    /// - `buffer_size`: the size of the raw data buffer in bytes
    pub unsafe fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        buffer_pointer: *const u8,
        buffer_size: u32,
    ) -> Self {
        Self(ClapEventMidiSysex {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventMidiSysex>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_MIDI_SYSEX,
                flags: event_flags.bits(),
            },
            port_index,
            buffer: buffer_pointer,
            size: buffer_size,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventMidiSysex) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventMidiSysex>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    /// The index of the MIDI port.
    pub fn port_index(&self) -> u16 {
        self.0.port_index
    }

    /// The raw MIDI data.
    pub fn data(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.buffer, self.0.size as usize) }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct EventMidi2(ClapEventMidi2);

impl EventMidi2 {
    /// Construct a new note event
    ///
    /// - `time`: sample offset within the buffer for this event
    /// - `space_id`: event_space, see clap_host_event_registry
    /// - `event_flags`: see `EventFlags`
    /// - `port_index`: the index of the MIDI2 port
    /// - `data`: the raw MIDI2 data
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        data: [u32; 4],
    ) -> Self {
        Self(ClapEventMidi2 {
            header: ClapEventHeader {
                size: std::mem::size_of::<ClapEventMidi2>() as u32,
                time,
                space_id,
                // Safe because this enum is represented with u16
                type_: clap_sys::events::CLAP_EVENT_MIDI2,
                flags: event_flags.bits(),
            },
            port_index,
            data,
        })
    }

    pub(crate) fn from_raw(mut event: ClapEventMidi2) -> Self {
        // I don't trust the plugin to always set this correctly.
        event.header.size = std::mem::size_of::<ClapEventMidi2>() as u32;

        Self(event)
    }

    /// Sample offset within the buffer for this event.
    pub fn time(&self) -> u32 {
        self.0.header.time
    }

    /// Event_space, see clap_host_event_registry.
    pub fn space_id(&self) -> u16 {
        self.0.header.space_id
    }

    pub fn event_flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.0.header.flags)
    }

    /// The index of the MIDI2 port.
    pub fn port_index(&self) -> u16 {
        self.0.port_index
    }

    /// The raw MIDI2 data.
    pub fn data(&self) -> [u32; 4] {
        self.0.data
    }
}
