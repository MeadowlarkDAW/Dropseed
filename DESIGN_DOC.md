# Dropseed Design Document

Dropseed is an open source audio graph engine, plugin hosting engine, system IO, and a general purpose DAW (Digital Audio Workstation) engine. It's main purpose is to be the engine powering [Meadowlark](https://github.com/MeadowlarkDAW/Meadowlark), but it can be used by other DAW projects and audio software that needs plugin hosting and/or an audio graph.

# Objective

Why am I creating a new DAW Engine? Why not just use the [Tracktion Engine](https://www.tracktion.com/develop/tracktion-engine)?

Dropseed aims to have these key features over Tracktion:

* Modular ecosystem: you only need to include the parts of the engine you use
* Good documentation with examples
* Written in Rust, with all the safety advantages that brings
* Zero dependencies on JUCE
* Completely independent of any GUI library
* Ability to interface with the engine in a serialized channel, allowing you to run the engine in a separate process
* Full first-class support for all of [CLAP]'s features, allowing for some exciting new ways to use plugins
* Better control over the engine: it doesn't force you to use a certain workflow
* C bindings
* *Maybe* MIT license? I haven't decided on that yet.

In addition, this is a passion project for me.

# Goals

### Audio Graph
* A highly flexible and robust audio graph engine that lets you route anything anywhere (as long as it doesn't create a cycle)
* Automatic summation
* Automatic delay compensation
* Automatic cycle detection
* High performance multithreaded processing
* Buffers and events are modeled after the [CLAP] spec, allowing it to take advantage of all of CLAP's features
* Some included nodes such as gain, pan, phase invert, wet/dry mix, mono-to-stereo, stereo-to-mono, monitor, etc
* Mixer node (includes gain, pan, mute, and phase invert for efficiency)
* Macro node (for controlling a parameter on a plugin)
    * Ability to be assigned to multiple parameters and plugins
    * Per-parameter ranges and curves
* Additional nodes such as stereo split, L/R split, and mulitband split
* Ability to create your own nodes using a similar API to [CLAP]
* (not MVP) Negative delay compensation
* (not MVP) 64-bit audio buffers

### Plugin Host
* First-class support for the open source [CLAP] audio plugin standard
* (not MVP) Support for LV2 and VST3 plugins via a CLAP bridge

### System IO
* Ability to enumerate and connect to system audio devices with low latency (including duplex support)
* Ability to enumerate and connect to system MIDI devices
* Robust error handling

### DAW Engine
* Transport
    * Multiple transports can exist in the same project
    * Loops
    * (not MVP) automated tempo
    * (not MVP) time signature changes
    * (not MVP) global swing
* Audio clip player node
    * An audio clip player node can house any number of clips
    * Automatic loading, resampling, and caching of audio data (all audio clip player nodes share the same cache)
    * Fades and crossfades
    * Clips can be assigned to multiple transports, as well as multiple places within the same transport
    * Clips can be played solo without a transport
    * Clips can be disabled
    * Automatic transport declicking, including loops
    * Reverse
    * Retrieve optimized waveform data for drawing waveforms in the UI
    * (not MVP) Ability to create your own custom offline audio clip effects
    * Record into a new clip
        * Retrieve optimized waveform data for drawing waveforms in the UI
        * Result of recording is saved to disk
        * (not MVP) Audio is streamed to disk as a backup
    * (not MVP) doppler stretching
    * (not MVP) high quality pitch and time stretching using 3rd party libraries like [Rubber Band Audio]
    * (not MVP) support for long audio clips via disk streaming
* Piano roll clip player node
    * A piano roll clip player node can house any number of clips
    * Clips can be assigned to multiple transports, as well as multiple places within the same transport
    * Clips can be played solo without a transport
    * Clips can be disabled
    * Supports [CLAP note events](https://github.com/free-audio/clap/blob/main/include/clap/events.h) and MIDI
    * Per-note expressions such as velocity, pan, timbre, pressure, etc.
    * Record into a new clip
    * (not MVP) custom tuning & microtonal scales
    * (not MVP) micro-pitch expressions
* Automation clip player node
    * An automation clip player node can house any number of clips
    * Clips can be assigned to multiple transports, as well as multiple places within the same transport
    * Clips can be disabled
    * Arbitrary number of automation points
    * Linear, Bezier, and step
    * Record into a new clip
    * (not MVP) more advanced curves such as sine waves
    * (not MVP) sample-accurate automation
* Recording node
    * Similar to the recorder in the audio clip player node, but can be placed anywhere in the graph. This allows you to do things like rendering a project to stems or bouncing a clip (pre or post FX).
* Sample browser playback node (used to quickly play back a sample as the user clicks it in the browser)
* (not MVP) Metronome node

### General
* Modular system that lets you choose what parts to include in your project
* (not MVP) Good documentation and examples
* (not MVP) C bindings

# Non-Goals

* No clip launcher (although it might be possible to do this with a custom node)
* VST3 and LV2 support will not receive the same level of support as CLAP plugins
* No support for the AUv2, AUv3, LADSPA, WAP (web audio plugin), or VCV Rack plugin formats
* No time and pitch stretching on long audio files that are streamed from disk
* Only exporting and recording audio to WAV will be supported
* If I do decide to go with the MIT license, then certain modules will need to fall under a separate license such as VST3 hosting, ASIO, and [Rubber Band Audio].

# Tech Stack

* system audio IO
    * Rust bindings to [RTAudio](https://github.com/thestk/rtaudio)
    * Or if that turns out to be too tricky, I'll try bindings to [miniaudio](https://crates.io/crates/om-fork-miniaudio) and [asio-sys](https://github.com/RustAudio/cpal/tree/master/asio-sys)
* system MIDI IO
    * [midir](https://crates.io/crates/midir)
* decoding audio files
    * [pcm-loader](https://github.com/MeadowlarkDAW/pcm-loader), which is my own wrapper around [Symphonia](https://github.com/pdeljanov/Symphonia)
* WAV encoding
    * [hound](https://crates.io/crates/hound)
* audio disk streaming
    * [creek](https://github.com/MeadowlarkDAW/creek)
* samplerate conversion
    * [rubato](https://crates.io/crates/rubato)
    * Or maybe bindings to [libsamplerate](https://github.com/MeadowlarkDAW/samplerate-rs), whichever one performs better
* CLAP plugin hosting
    * [clack](https://github.com/prokopyl/clack)
    * Or if that doesn't work out, raw bindings with [clap-sys](https://github.com/prokopyl/clap-sys)
* LV2 plugin hosting
    * [livi-rs](https://github.com/wmedrano/livi-rs)
* VST3 plugin hosting
    * [vst3-sys](https://github.com/RustAudio/vst3-sys)
* Pitch shifting and time stretching
    * Bindings to [Rubber Band Audio]

# Architecture

*TODO*

[CLAP]: https://github.com/free-audio/clap
[Rubber Band Audio]: https://www.rubberbandaudio.com/