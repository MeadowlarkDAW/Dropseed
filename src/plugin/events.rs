//! This is a direct 1:1 port of the events structure as specified in the CLAP plugin spec.
//! <https://github.com/free-audio/clap/blob/main/include/clap/events.h>
//!
//! I chose to use the raw C format rather than a "rusty" version of it so we won't have to
//! do any (potentially expensive) type conversions on the fly going to/from CLAP plugins.
//!
//! Note I have chosen to "rust-ify" other non-performance critical aspects of
//! the spec such as plugin description and audio port configuration.

use bitflags::bitflags;

use crate::FixedPoint64;

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
pub enum EventType {
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

/// The header of an event.
///
/// This must be the first attribute of every event.
#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventHeader {
    /// Event size including this header (in bytes).
    pub size: u32,
    /// Time at which the event happens.
    pub time: u32,
    /// Event space, see clap_host_event_registry. (TODO)
    pub space_id: u16,
    pub event_type: EventType,
    pub flags: EventFlags,
}

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

    /// -1 if unspecified, otherwise >0
    pub note_id: i32,

    pub port_index: i16,

    /// [0..15]
    pub channel: i16,

    /// [0..127]
    pub key: i16,

    /// [0.0, 1.0]
    pub velocity: f64,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteExpression {
    /// with 0.0 < x <= 4.0, plain = 20.0 * log(x)
    Volume,
    /// pan, 0.0 left, 0.5 center, 1.0 right
    Pan,
    /// Relative tuning in semitone, from -120.0 to +120.0
    Tuning,
    /// [0.0, 1.0]
    Vibrato,
    /// [0.0, 1.0]
    Expression,
    /// [0.0, 1.0]
    Brightness,
    /// [0.0, 1.0]
    Pressure,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventNoteExpression {
    pub header: EventHeader,

    pub expression_id: NoteExpression,

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

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamValue {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: u32,

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

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamMod {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: u32,

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

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventParamGesture {
    pub header: EventHeader,

    /// Target parameter.
    pub param_id: u32,
}

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

    pub flags: TransportFlags,

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

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidi {
    pub header: EventHeader,

    pub port_index: u16,
    pub data: [u8; 3],
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidiSysex {
    pub header: EventHeader,

    pub port_index: u16,

    /// Raw Midi buffer
    pub buffer: *const u8,
    /// Size of the `buffer` in bytes.
    pub size: u32,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct EventMidi2 {
    pub header: EventHeader,

    pub port_index: u16,
    pub data: [u32; 4],
}
