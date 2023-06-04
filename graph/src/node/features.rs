//! This file provides a set of standard features meant to be used in `NodeDescriptor`.

// -- Category ------------------------------------------------------------------------------

/// Add this feature if your node can process note events and then produce audio.
pub static INSTRUMENT: &'static str = "instrument";

/// Add this feature if your node is an audio effect.
pub static AUDIO_EFFECT: &'static str = "audio-effect";

/// Add this feature if your node is a note effect or a note generator/sequencer.
pub static NOTE_EFFECT: &'static str = "note-effect";

/// Add this feature if your plugin converts audio to notes.
pub static NOTE_DETECTOR: &'static str = "note-detector";

/// Add this feature if your plugin is an analyzer.
pub static ANALYZER: &'static str = "analyzer";

// -- Sub-category ------------------------------------------------------------------------------

pub static SYNTHESIZER: &'static str = "synthesizer";
pub static SAMPLER: &'static str = "sampler";
/// For single drum
pub static DRUM: &'static str = "drum";
pub static DRUM_MACHINE: &'static str = "drum-machine";

pub static FILTER: &'static str = "filter";
pub static PHASER: &'static str = "phaser";
pub static EQUALIZER: &'static str = "equalizer";
pub static DEESSER: &'static str = "de-esser";
pub static PHASE_VOCODER: &'static str = "phase-vocoder";
pub static GRANULAR: &'static str = "granular";
pub static FREQUENCY_SHIFTER: &'static str = "frequency-shifter";
pub static PITCH_SHIFTER: &'static str = "pitch-shifter";

pub static DISTORTION: &'static str = "distortion";
pub static TRANSIENT_SHAPER: &'static str = "transient-shaper";
pub static COMPRESSOR: &'static str = "compressor";
pub static EXPANDER: &'static str = "expander";
pub static GATE: &'static str = "gate";
pub static LIMITER: &'static str = "limiter";

pub static FLANGER: &'static str = "flanger";
pub static CHORUS: &'static str = "chorus";
pub static DELAY: &'static str = "delay";
pub static REVERB: &'static str = "reverb";

pub static TREMELO: &'static str = "tremelo";
pub static GLITCH: &'static str = "glitch";

pub static UTILITY: &'static str = "utility";
pub static PITCH_CORRECTION: &'static str = "pitch-correction";
pub static RESTORATION: &'static str = "restoration";

pub static MULTI_EFFECTS: &'static str = "multi-effects";

pub static MIXING: &'static str = "mixing";
pub static MASTERING: &'static str = "mastering";

// -- Audio capabilities ------------------------------------------------------------------------------

pub static MONO: &'static str = "mono";
pub static STEREO: &'static str = "stereo";
pub static SURROUND: &'static str = "surround";
pub static AMBISONIC: &'static str = "ambisonic";
