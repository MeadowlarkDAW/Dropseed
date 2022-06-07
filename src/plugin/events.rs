//! This is a direct 1:1 port of the events structure as specified in the CLAP plugin spec.
//! <https://github.com/free-audio/clap/blob/main/include/clap/events.h>
//!
//! I chose to use the raw C format rather than a "rusty" version of it so we won't have to
//! do any (potentially expensive) type conversions on the fly going to/from CLAP plugins.
//!
//! Note I have chosen to "rust-ify" other non-performance critical aspects of
//! the spec such as plugin description and audio port configuration.

use bitflags::bitflags;

use crate::{FixedPoint64, ParamID};

pub enum PluginEvent<'a> {
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

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEventType {
    /// Represents a key pressed event.
    NoteOn = 0,
    /// Represents a key released event.
    NoteOff = 1,
    /// This is meant to choke the voice(s), like in a drum machine when a closed hihat
    /// chokes an open hihat.
    NoteChoke = 2,
    /// This is sent by the plugin to the host. The port, channel and key are those given
    /// by the host in the `NoteOn` event. In other words, this event is matched against the
    /// plugin's note input port. `NoteEnd` is only requiered if the plugin marked at least
    /// one of its parameters as polyphonic.
    ///
    /// When using polyphonic modulations, the host has to allocate and release voices for its
    /// polyphonic modulator. Yet only the plugin effectively knows when the host should terminate
    /// a voice. `NoteEnd` solves that issue in a non-intrusive and cooperative way.
    ///
    /// This assumes that the host will allocate a unique voice on a `NoteOn` event for a given port,
    /// channel and key. This voice will run until the plugin will instruct the host to terminate
    /// it by sending a `NoteEnd` event.
    ///
    /// Consider the following sequence:
    /// - process()
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:16, time:t0)
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:64, time:t0)
    ///    - Host->Plugin NoteOff(port:0, channel:0, key:16, t1)
    ///    - Host->Plugin NoteOff(port:0, channel:0, key:64, t1)
    ///         - on t2, both notes did terminate
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:64, t3)
    ///         - Here the plugin finished to process all the frames and will tell the host
    ///         to terminate the voice on key 16 but not 64, because a note has been started at t3
    ///    - Plugin->Host NoteEnd(port:0, channel:0, key:16, time:ignored)
    NoteEnd = 3,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamGestureEventType {
    /// Indicates that a parameter gesture has begun.
    ParamGestureBegin = 7,
    /// Indicates that a parameter gesture has ended.
    ParamGestureEnd = 8,
}

/// Some of the following events overlap, a note on can be expressed with:
/// - `EventType::NoteOn`
/// - `EventType::Midi`
/// - `EventType::Midi2`
///
/// The preferred way of sending a note event is to use `EventType::Note*`.
///
/// The same event must not be sent twice: it is forbidden to send the same note on
/// encoded with both `EventType::NoteOn` and `EventType::Midi`.
///
/// The plugins are encouraged to be able to handle note events encoded as raw midi or midi2,
/// or implement clap_plugin_event_filter and reject raw midi and midi2 events.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventType {
    /// Represents a key pressed event.
    NoteOn = 0,
    /// Represents a key released event.
    NoteOff = 1,
    /// This is meant to choke the voice(s), like in a drum machine when a closed hihat
    /// chokes an open hihat.
    NoteChoke = 2,
    /// This is sent by the plugin to the host. The port, channel and key are those given
    /// by the host in the `NoteOn` event. In other words, this event is matched against the
    /// plugin's note input port. `NoteEnd` is only requiered if the plugin marked at least
    /// one of its parameters as polyphonic.
    ///
    /// When using polyphonic modulations, the host has to allocate and release voices for its
    /// polyphonic modulator. Yet only the plugin effectively knows when the host should terminate
    /// a voice. `NoteEnd` solves that issue in a non-intrusive and cooperative way.
    ///
    /// This assumes that the host will allocate a unique voice on a `NoteOn` event for a given port,
    /// channel and key. This voice will run until the plugin will instruct the host to terminate
    /// it by sending a `NoteEnd` event.
    ///
    /// Consider the following sequence:
    /// - process()
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:16, time:t0)
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:64, time:t0)
    ///    - Host->Plugin NoteOff(port:0, channel:0, key:16, t1)
    ///    - Host->Plugin NoteOff(port:0, channel:0, key:64, t1)
    ///         - on t2, both notes did terminate
    ///    - Host->Plugin NoteOn(port:0, channel:0, key:64, t3)
    ///         - Here the plugin finished to process all the frames and will tell the host
    ///         to terminate the voice on key 16 but not 64, because a note has been started at t3
    ///    - Plugin->Host NoteEnd(port:0, channel:0, key:16, time:ignored)
    NoteEnd = 3,

    /// Represents a note expression.
    NoteExpression = 4,

    /// `ParamValue` sets the parameter's value.
    ///
    /// The value heard is: param_value + param_mod.
    ///
    /// In case of a concurrent global value/modulation versus a polyphonic one,
    /// the voice should only use the polyphonic one and the polyphonic modulation
    /// amount will already include the monophonic signal.
    ParamValue = 5,
    /// `ParamMod` sets the parameter's modulation amount.
    ///
    /// The value heard is: param_value + param_mod.
    ///
    /// In case of a concurrent global value/modulation versus a polyphonic one,
    /// the voice should only use the polyphonic one and the polyphonic modulation
    /// amount will already include the monophonic signal.
    ParamMod = 6,

    /// Indicates that a parameter gesture has begun.
    ParamGestureBegin = 7,
    /// Indicates that a parameter gesture has ended.
    ParamGestureEnd = 8,

    /// Update the transport info.
    Transport = 9,
    /// Raw midi event.
    Midi = 10,
    /// Raw midi sysex event.  
    MidiSysex = 11,
    /// Raw midi 2 event.
    Midi2 = 12,
}

impl EventType {
    #[inline]
    pub fn from_u16(val: u16) -> Option<Self> {
        match val {
            0 => Some(EventType::NoteOn),
            1 => Some(EventType::NoteOff),
            2 => Some(EventType::NoteChoke),
            3 => Some(EventType::NoteEnd),
            4 => Some(EventType::NoteExpression),
            5 => Some(EventType::ParamValue),
            6 => Some(EventType::ParamMod),
            7 => Some(EventType::ParamGestureBegin),
            8 => Some(EventType::ParamGestureEnd),
            9 => Some(EventType::Transport),
            10 => Some(EventType::Midi),
            11 => Some(EventType::MidiSysex),
            12 => Some(EventType::Midi2),
            _ => None,
        }
    }

    #[inline]
    pub fn as_u16(&self) -> u16 {
        match self {
            EventType::NoteOn => 0,
            EventType::NoteOff => 1,
            EventType::NoteChoke => 2,
            EventType::NoteEnd => 3,
            EventType::NoteExpression => 4,
            EventType::ParamValue => 5,
            EventType::ParamMod => 6,
            EventType::ParamGestureBegin => 7,
            EventType::ParamGestureEnd => 8,
            EventType::Transport => 9,
            EventType::Midi => 10,
            EventType::MidiSysex => 11,
            EventType::Midi2 => 12,
        }
    }
}

impl From<NoteEventType> for EventType {
    fn from(e: NoteEventType) -> Self {
        match e {
            NoteEventType::NoteOn => EventType::NoteOn,
            NoteEventType::NoteOff => EventType::NoteOff,
            NoteEventType::NoteChoke => EventType::NoteChoke,
            NoteEventType::NoteEnd => EventType::NoteEnd,
        }
    }
}

impl From<ParamGestureEventType> for EventType {
    fn from(e: ParamGestureEventType) -> Self {
        match e {
            ParamGestureEventType::ParamGestureBegin => EventType::ParamGestureBegin,
            ParamGestureEventType::ParamGestureEnd => EventType::ParamGestureEnd,
        }
    }
}

/// The header of an event.
///
/// This must be the first attribute of every event.
#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventHeader {
    /// Event size including this header (in bytes).
    pub(crate) size: u32,
    /// Time at which the event happens.
    pub time: u32,
    /// Event space, see clap_host_event_registry. (TODO)
    pub space_id: u16,
    event_type: u16,
    flags: u32,
}

impl EventHeader {
    pub(crate) fn event_type(&self) -> Option<EventType> {
        EventType::from_u16(self.event_type)
    }

    pub fn flags(&self) -> EventFlags {
        EventFlags::from_bits_truncate(self.flags)
    }
}

unsafe impl bytemuck::Pod for EventHeader {}
unsafe impl bytemuck::Zeroable for EventHeader {}

bitflags! {
    pub struct EventFlags: u32 {
        /// Indicate a live momentary event.
        const IS_LIVE = 1 << 0;

        /// Indicate that the event should not be recorded.
        ///
        /// For example this is useful when a parameter changes because of a MIDI CC,
        /// because if the host records both the MIDI CC automation and the parameter
        /// automation there will be a conflict.
        const CLAP_EVENT_DONT_RECORD = 1 << 1;
    }
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventNote {
    pub header: EventHeader,

    /// Target a specific note id, or -1 for global.
    pub note_id: i32,

    pub port_index: i16,

    /// [0..15]
    pub channel: i16,

    /// [0..127]
    pub key: i16,

    /// [0.0, 1.0]
    pub velocity: f64,
}

impl EventNote {
    /// - time: Time at which the event happens.
    /// - note_id: Target a specific note id, or -1 for global.
    /// - port_index: Target a specific port index, or -1 for global.
    /// - channel: Target a specific channel, or -1 for global.
    /// - key: Target a specific key, or -1 for global.
    /// - velocity: [0.0, 1.0]
    pub fn new(
        time: u32,
        space_id: u16,
        event_type: NoteEventType,
        event_flags: EventFlags,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        velocity: f64,
    ) -> Self {
        let event_type: EventType = event_type.into();

        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventNote>() as u32,
                time,
                space_id,
                event_type: event_type.as_u16(),
                flags: event_flags.bits(),
            },
            note_id,
            port_index,
            channel,
            key,
            velocity,
        }
    }
}

unsafe impl bytemuck::Pod for EventNote {}
unsafe impl bytemuck::Zeroable for EventNote {}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExpression {
    /// with 0.0 < x <= 4.0, plain = 20.0 * log(x)
    Volume = 0,
    /// pan, 0.0 left, 0.5 center, 1.0 right
    Pan = 1,
    /// Relative tuning in semitone, from -120.0 to +120.0
    Tuning = 2,
    /// [0.0, 1.0]
    Vibrato = 3,
    /// [0.0, 1.0]
    Expression = 4,
    /// [0.0, 1.0]
    Brightness = 5,
    /// [0.0, 1.0]
    Pressure = 6,
}

impl NoteExpression {
    #[inline]
    pub fn from_i32(val: i32) -> Option<Self> {
        match val {
            0 => Some(NoteExpression::Volume),
            1 => Some(NoteExpression::Pan),
            2 => Some(NoteExpression::Tuning),
            3 => Some(NoteExpression::Vibrato),
            4 => Some(NoteExpression::Expression),
            5 => Some(NoteExpression::Brightness),
            6 => Some(NoteExpression::Pressure),
            _ => None,
        }
    }

    #[inline]
    pub fn as_i32(&self) -> i32 {
        match self {
            NoteExpression::Volume => 0,
            NoteExpression::Pan => 1,
            NoteExpression::Tuning => 2,
            NoteExpression::Vibrato => 3,
            NoteExpression::Expression => 4,
            NoteExpression::Brightness => 5,
            NoteExpression::Pressure => 6,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventNoteExpression {
    pub header: EventHeader,

    /// Use `NoteExpression::from_i32()` to extract this value.
    pub(crate) expression_id: i32,

    /// Target a specific note id, or -1 for global.
    pub note_id: i32,
    /// Target a specific port index, or -1 for global.
    pub port_index: i16,
    /// Target a specific channel, or -1 for global.
    pub channel: i16,
    /// Target a specific key, or -1 for global.
    pub key: i16,

    /// See `NoteExpression` for the range.
    pub value: f64,
}

impl EventNoteExpression {
    pub fn expression_id(&self) -> Option<NoteExpression> {
        NoteExpression::from_i32(self.expression_id)
    }

    /// - time: Time at which the event happens.
    /// - note_id: Target a specific note id, or -1 for global.
    /// - port_index: Target a specific port index, or -1 for global.
    /// - channel: Target a specific channel, or -1 for global.
    /// - key: Target a specific key, or -1 for global.
    /// - value: See `NoteExpression` for the range.
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        expression_id: NoteExpression,
        note_id: i32,
        port_index: i16,
        channel: i16,
        key: i16,
        value: f64,
    ) -> Self {
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventNoteExpression>() as u32,
                time,
                space_id,
                event_type: EventType::NoteExpression.as_u16(),
                flags: event_flags.bits(),
            },
            expression_id: expression_id.as_i32(),
            note_id,
            port_index,
            channel,
            key,
            value,
        }
    }
}

unsafe impl bytemuck::Pod for EventNoteExpression {}
unsafe impl bytemuck::Zeroable for EventNoteExpression {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamValue {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: ParamID,

    /// (Reserved for the CLAP plugin host)
    pub _cookie: *const std::ffi::c_void,

    /// Target a specific note id, or -1 for global.
    pub note_id: i32,
    /// Target a specific port index, or -1 for global.
    pub port_index: i16,
    /// Target a specific channel, or -1 for global.
    pub channel: i16,
    /// Target a specific key, or -1 for global.
    pub key: i16,

    pub value: f64,
}

impl EventParamValue {
    /// - time: Time at which the event happens.
    ///
    /// - param_id: Target parameter.
    /// - note_id: Target a specific note id, or -1 for global.
    /// - port_index: Target a specific port index, or -1 for global.
    /// - channel: Target a specific channel, or -1 for global.
    /// - key: Target a specific key, or -1 for global.
    /// - value: See `NoteExpression` for the range.
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
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventParamValue>() as u32,
                time,
                space_id,
                event_type: EventType::ParamValue.as_u16(),
                flags: event_flags.bits(),
            },
            param_id,
            _cookie: std::ptr::null(),
            note_id,
            port_index,
            channel,
            key,
            value,
        }
    }
}

unsafe impl bytemuck::Pod for EventParamValue {}
unsafe impl bytemuck::Zeroable for EventParamValue {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamMod {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: ParamID,

    /// (Reserved for the CLAP plugin host)
    pub _cookie: *const std::ffi::c_void,

    /// Target a specific note id, or -1 for global.
    pub note_id: i32,
    /// Target a specific port index, or -1 for global.
    pub port_index: i16,
    /// Target a specific channel, or -1 for global.
    pub channel: i16,
    /// Target a specific key, or -1 for global.
    pub key: i16,

    /// Modulation amount.
    pub amount: f64,
}

impl EventParamMod {
    /// - time: Time at which the event happens.
    /// - param_id: Target parameter.
    /// - note_id: Target a specific note id, or -1 for global.
    /// - port_index: Target a specific port index, or -1 for global.
    /// - channel: Target a specific channel, or -1 for global.
    /// - key: Target a specific key, or -1 for global.
    /// - amount: Modulation amount.
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
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventParamMod>() as u32,
                time,
                space_id,
                event_type: EventType::ParamMod.as_u16(),
                flags: event_flags.bits(),
            },
            param_id,
            _cookie: std::ptr::null(),
            note_id,
            port_index,
            channel,
            key,
            amount,
        }
    }
}

unsafe impl bytemuck::Pod for EventParamMod {}
unsafe impl bytemuck::Zeroable for EventParamMod {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamGesture {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: ParamID,
}

impl EventParamGesture {
    /// - time: Time at which the event happens.
    /// - param_id: Target parameter.
    pub fn new(
        time: u32,
        space_id: u16,
        event_type: ParamGestureEventType,
        event_flags: EventFlags,
        param_id: ParamID,
    ) -> Self {
        let event_type: EventType = event_type.into();

        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventParamGesture>() as u32,
                time,
                space_id,
                event_type: event_type.as_u16(),
                flags: event_flags.bits(),
            },
            param_id,
        }
    }

    pub fn is_begin(&self) -> bool {
        if let Some(event_type) = EventType::from_u16(self.header.event_type) {
            event_type == EventType::ParamGestureBegin
        } else {
            false
        }
    }
}

unsafe impl bytemuck::Pod for EventParamGesture {}
unsafe impl bytemuck::Zeroable for EventParamGesture {}

bitflags! {
    pub struct TransportFlags: u32 {
        const HAS_TEMPO = 1 << 0;
        const HAS_BEATS_TIMELINE = 1 << 1;
        const HAS_SECONDS_TIMELINE = 1 << 2;
        const HAS_TIME_SIGNATURE = 1 << 3;
        const IS_PLAYING = 1 << 4;
        const IS_RECORDING = 1 << 5;
        const IS_LOOP_ACTIVE = 1 << 6;
        const IS_WITHIN_PRE_ROLL = 1 << 7;
    }
}

pub type BeatTime = FixedPoint64;
pub type SecTime = FixedPoint64;

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventTransport {
    pub header: EventHeader,

    pub(crate) flags: u32,

    /// Position in beats.
    pub song_pos_beats: BeatTime,
    /// Position in seconds.
    pub song_pos_seconds: SecTime,

    /// The tempo in bpm.
    pub tempo: f64,
    /// Tempo increment for each samples and until the next
    /// time info event.
    pub tempo_inc: f64,

    pub loop_start_beats: BeatTime,
    pub loop_end_beats: BeatTime,
    pub loop_start_seconds: SecTime,
    pub loop_end_seconds: SecTime,

    /// Start pos of the current bar.
    pub bar_start: BeatTime,
    /// Bar at song pos 0 has the number 0.
    pub bar_number: i32,

    /// Time signature numerator.
    pub tsig_num: i16,
    /// Time signature denominator.
    pub tsig_denom: i16,
}

impl EventTransport {
    pub fn transport_flags(&self) -> TransportFlags {
        TransportFlags::from_bits_truncate(self.flags)
    }

    /// - time: Time at which the event happens.
    /// - song_pos_beats: Position in beats.
    /// - song_pos_seconds: Position in seconds.
    /// - tempo: The tempo in bpm.
    /// - tempo_inc: Tempo increment for each samples and until the next time info event.
    /// - bar_start: Start pos of the current bar.
    /// - bar_number: Bar at song pos 0 has the number 0.
    /// - tsig_num: Time signature numerator.
    /// - tsig_denom: Time signature denominator.
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        transport_flags: TransportFlags,
        song_pos_beats: BeatTime,
        song_pos_seconds: SecTime,
        tempo: f64,
        tempo_inc: f64,
        loop_start_beats: BeatTime,
        loop_end_beats: BeatTime,
        loop_start_seconds: SecTime,
        loop_end_seconds: SecTime,
        bar_start: BeatTime,
        bar_number: i32,
        tsig_num: i16,
        tsig_denom: i16,
    ) -> Self {
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventTransport>() as u32,
                time,
                space_id,
                event_type: EventType::Transport.as_u16(),
                flags: event_flags.bits(),
            },
            flags: transport_flags.bits(),
            song_pos_beats,
            song_pos_seconds,
            tempo,
            tempo_inc,
            loop_start_beats,
            loop_end_beats,
            loop_start_seconds,
            loop_end_seconds,
            bar_start,
            bar_number,
            tsig_num,
            tsig_denom,
        }
    }
}

unsafe impl bytemuck::Pod for EventTransport {}
unsafe impl bytemuck::Zeroable for EventTransport {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidi {
    pub header: EventHeader,

    pub port_index: u16,
    pub data: [u8; 3],
}

impl EventMidi {
    /// - time: Time at which the event happens.
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        data: [u8; 3],
    ) -> Self {
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventMidi>() as u32,
                time,
                space_id,
                event_type: EventType::Midi.as_u16(),
                flags: event_flags.bits(),
            },
            port_index,
            data,
        }
    }
}

unsafe impl bytemuck::Pod for EventMidi {}
unsafe impl bytemuck::Zeroable for EventMidi {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidiSysex {
    pub header: EventHeader,

    pub port_index: u16,

    /// Raw Midi buffer.
    pub buffer: *const u8,
    /// Size of `buffer` in bytes.
    pub size: u32,
}

impl EventMidiSysex {
    /// - time: Time at which the event happens.
    /// - buffer: Raw Midi buffer.
    /// - size: Size of `buffer` in bytes.
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        buffer: *const u8,
        size: u32,
    ) -> Self {
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventMidiSysex>() as u32,
                time,
                space_id,
                event_type: EventType::MidiSysex.as_u16(),
                flags: event_flags.bits(),
            },
            port_index,
            buffer,
            size,
        }
    }
}

unsafe impl bytemuck::Pod for EventMidiSysex {}
unsafe impl bytemuck::Zeroable for EventMidiSysex {}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidi2 {
    pub header: EventHeader,

    pub port_index: u16,
    pub data: [u32; 4],
}

impl EventMidi2 {
    /// - time: Time at which the event happens.
    /// - buffer: Raw Midi buffer.
    /// - size: Size of `buffer` in bytes.
    pub fn new(
        time: u32,
        space_id: u16,
        event_flags: EventFlags,
        port_index: u16,
        data: [u32; 4],
    ) -> Self {
        Self {
            header: EventHeader {
                size: std::mem::size_of::<EventMidi2>() as u32,
                time,
                space_id,
                event_type: EventType::Midi2.as_u16(),
                flags: event_flags.bits(),
            },
            port_index,
            data,
        }
    }
}

unsafe impl bytemuck::Pod for EventMidi2 {}
unsafe impl bytemuck::Zeroable for EventMidi2 {}
