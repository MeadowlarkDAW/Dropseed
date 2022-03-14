use super::AudioPortBuffer;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentBufferLayout {
    StereoOut32,
    StereoOut64,

    MonoOut32,
    MonoOut64,

    StereoInPlace32,
    StereoInPlace64,

    StereoInPlaceWithSidechain32,
    StereoInPlaceWithSidechain64,

    StereoInPlaceWithExtraOut32,
    StereoInPlaceWithExtraOut64,

    MonoInPlace32,
    MonoInPlace64,

    MonoInPlaceWithSidechain32,
    MonoInPlaceWithSidechain64,

    StereoInOut32,
    StereoInOut64,

    StereoInOutWithSidechain32,
    StereoInOutWithSidechain64,

    StereoInOutWithExtraOut32,
    StereoInOutWithExtraOut64,

    MonoInOut32,
    MonoInOut64,

    MonoInOutWithSidechain32,
    MonoInOutWithSidechain64,

    MonoInStereoOut32,
    MonoInStereoOut64,

    StereoInMonoOut32,
    StereoInMonoOut64,

    /* TODO
    SurroundOut32,
    SurroundOut64,

    SurroundInPlace32,
    SurroundInPlace64,

    SurroundInPlaceWithSidechain32,
    SurroundInPlaceWithSidechain64,

    SurroundInOut32,
    SurroundInOut64,

    SurroundInOutWithSidechainF2,
    SurroundInOutWithSidechain64,
    */
    Custom,
}

/// The layout of audio buffers sent to this plugin's `process()` method.
///
/// The host will always send the same
#[non_exhaustive]
pub enum ProcBufferLayout<'a> {
    /// For plugins that use `AudioPortLayout::StereoOut`, the host will always
    /// send this.
    ///
    /// For plugins that use `AudioPortLayout::StereoOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoOut32`
    /// * `ProcBufferLayout::StereoOut64`
    StereoOut32 { left: &'a mut [f32], right: &'a mut [f32] },
    /// For plugins that use `AudioPortLayout::StereoOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoOut32`
    /// * `ProcBufferLayout::StereoOut64`
    StereoOut64 { left: &'a mut [f64], right: &'a mut [f64] },

    /// For plugins that use `AudioPortLayout::MonoOut`, the host will always
    /// send this.
    ///
    /// For plugins that use `AudioPortLayout::MonoOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoOut32`
    /// * `ProcBufferLayout::MonoOut64`
    MonoOut32(&'a mut [f32]),
    /// For plugins that use `AudioPortLayout::MonoOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoOut32`
    /// * `ProcBufferLayout::MonoOut64`
    MonoOut64(&'a mut [f64]),

    /// For plugins that use `AudioPortLayout::StereoInPlace`, the host may send
    /// one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace64`
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInPlace32 { left: &'a mut [f32], right: &'a mut [f32] },
    /// For plugins that use `AudioPortLayout::StereoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace64`
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInPlace64 { left: &'a mut [f64], right: &'a mut [f64] },

    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechain`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInPlaceWithSidechain32 {
        left: &'a mut [f32],
        right: &'a mut [f32],

        sc_left: &'a [f32],
        sc_right: &'a [f32],
    },
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInPlaceWithSidechain64 {
        left: &'a mut [f64],
        right: &'a mut [f64],

        sc_left: &'a [f64],
        sc_right: &'a [f64],
    },

    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOut`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInPlaceWithExtraOut32 {
        left: &'a mut [f32],
        right: &'a mut [f32],

        extra_out_left: &'a mut [f32],
        extra_out_right: &'a mut [f32],
    },
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInPlaceWithExtraOut64 {
        left: &'a mut [f64],
        right: &'a mut [f64],

        extra_out_left: &'a mut [f64],
        extra_out_right: &'a mut [f64],
    },

    /// For plugins that use `AudioPortLayout::MonoInPlace`, the host may send
    /// one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace64`
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInPlace32(&'a mut [f32]),
    /// For plugins that use `AudioPortLayout::MonoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace64`
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInPlace64(&'a mut [f64]),

    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechain`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MononOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInPlaceWithSidechain32 { in_out: &'a mut [f32], sc: &'a [f32] },
    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInPlaceWithSidechain64 { in_out: &'a mut [f64], sc: &'a [f64] },

    /// For plugins that use `AudioPortLayout::StereoInOut`, the host will always
    /// send this.
    ///
    /// For plugins that use `AudioPortLayout::StereoInOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlace`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace64`
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInOut32 {
        in_left: &'a [f32],
        in_right: &'a [f32],
        out_left: &'a mut [f32],
        out_right: &'a mut [f32],
    },
    /// For plugins that use `AudioPortLayout::StereoInOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlace64`
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInOut64 {
        in_left: &'a [f64],
        in_right: &'a [f64],
        out_left: &'a mut [f64],
        out_right: &'a mut [f64],
    },

    /// For plugins that use `AudioPortLayout::StereoInOutWithSidechain`, the host will
    /// always send this.
    ///
    /// For plugins that use `AudioPortLayout::StereoInOutWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechain`, the host
    /// may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechainPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInOutWithSidechain32 {
        in_left: &'a [f32],
        in_right: &'a [f32],

        out_left: &'a mut [f32],
        out_right: &'a mut [f32],

        sc_left: &'a [f32],
        sc_right: &'a [f32],
    },
    /// For plugins that use `AudioPortLayout::StereoInOutWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithSidechainPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInOutWithSidechain64 {
        in_left: &'a [f64],
        in_right: &'a [f64],

        out_left: &'a mut [f64],
        out_right: &'a mut [f64],

        sc_left: &'a [f64],
        sc_right: &'a [f64],
    },

    /// For plugins that use `AudioPortLayout::StereoInOutWithExtraOut`, the host will
    /// always send this.
    ///
    /// For plugins that use `AudioPortLayout::StereoInOutWithExtraOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOut`, the host
    /// may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOutPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInOutWithExtraOut32 {
        in_left: &'a [f32],
        in_right: &'a [f32],

        out_left: &'a mut [f32],
        out_right: &'a mut [f32],

        extra_out_left: &'a mut [f32],
        extra_out_right: &'a mut [f32],
    },
    /// For plugins that use `AudioPortLayout::StereoInOutWithExtraOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    ///
    /// For plugins that use `AudioPortLayout::StereoInPlaceWithExtraOutPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInOutWithExtraOut64 {
        in_left: &'a [f64],
        in_right: &'a [f64],

        out_left: &'a mut [f64],
        out_right: &'a mut [f64],

        extra_out_left: &'a mut [f64],
        extra_out_right: &'a mut [f64],
    },

    /// For plugins that use `AudioPortLayout::MonoInOut`, the host will always
    /// send this.
    ///
    /// For plugins that use `AudioPortLayout::MonoInOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlace`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace64`
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInOut32 { input: &'a [f32], output: &'a mut [f32] },
    /// For plugins that use `AudioPortLayout::MonoInOutPrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInOut32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlacePrefers64`, the host may
    /// send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlace64`
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInOut64 { input: &'a [f64], output: &'a mut [f64] },

    /// For plugins that use `AudioPortLayout::MonoInOutWithSidechain`, the host will
    /// always send this.
    ///
    /// For plugins that use `AudioPortLayout::MonoInOutWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechain`, the host
    /// may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechainPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInOutWithSidechain32 { input: &'a [f32], output: &'a mut [f32], sc: &'a [f32] },
    /// For plugins that use `AudioPortLayout::MonoInOutWithSidechainPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    ///
    /// For plugins that use `AudioPortLayout::MonoInPlaceWithSidechainPrefers64`,
    /// the host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInOutWithSidechain64 { input: &'a [f64], output: &'a mut [f64], sc: &'a [f64] },

    /// For plugins that use `AudioPortLayout::MonoInStereoOut`, the host will
    /// always send this.
    ///
    /// For plugins that use `AudioPortLayout::MonoInStereoOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInStereoOut64`
    /// * `ProcBufferLayout::MonoInStereoOut32`
    MonoInStereoOut32 { input: &'a [f32], out_left: &'a mut [f32], out_right: &'a mut [f32] },
    /// For plugins that use `AudioPortLayout::MonoInStereoOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::MonoInStereoOut64`
    /// * `ProcBufferLayout::MonoInStereoOut32`
    MonoInStereoOut64 { input: &'a [f64], out_left: &'a mut [f64], out_right: &'a mut [f64] },

    /// For plugins that use `AudioPortLayout::StereoInMonoOut`, the host will
    /// always send this.
    ///
    /// For plugins that use `AudioPortLayout::StereoInMonoOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInMonoOut64`
    /// * `ProcBufferLayout::StereoInMonoOut32`
    StereoInMonoOut32 { in_left: &'a [f32], in_right: &'a [f32], output: &'a mut [f32] },
    /// For plugins that use `AudioPortLayout::StereoInMonoOutPrefers64`, the
    /// host may send one of these:
    ///
    /// * `ProcBufferLayout::StereoInMonoOut64`
    /// * `ProcBufferLayout::StereoInMonoOut32`
    StereoInMonoOut64 { in_left: &'a [f64], in_right: &'a [f64], output: &'a mut [f64] },

    /* TODO
    SurroundOut32(&'a mut AudioPortBuffer<f32>),
    SurroundOut64(&'a mut AudioPortBuffer<f64>),

    SurroundInPlace32(&'a mut AudioPortBuffer<f32>),
    SurroundInPlace64(&'a mut AudioPortBuffer<f64>),

    SurroundInPlaceWithSidechain32 {
        in_out: &'a mut AudioPortBuffer<f32>,

        sc: &'a AudioPortBuffer<f32>,
    },
    SurroundInPlaceWithSidechain64 {
        in_out: &'a mut AudioPortBuffer<f64>,

        sc: &'a AudioPortBuffer<f64>,
    },

    SurroundInOut32 {
        input: &'a AudioPortBuffer<f32>,
        output: &'a mut AudioPortBuffer<f32>,
    },
    SurroundInOut64 {
        input: &'a AudioPortBuffer<f64>,
        output: &'a mut AudioPortBuffer<f64>,
    },

    SurroundInOutWithSidechain32 {
        input: &'a AudioPortBuffer<f32>,
        output: &'a mut AudioPortBuffer<f32>,

        sc: &'a AudioPortBuffer<f32>,
    },
    SurroundInOutWithSidechain64 {
        input: &'a AudioPortBuffer<f64>,
        output: &'a mut AudioPortBuffer<f64>,

        sc: &'a AudioPortBuffer<f64>,
    },
    */
    /// For plugins that use `AudioPortLayout::Custom`, the host will
    /// always send this.
    Custom(RawBufferLayout<'a>),
}

/// The raw audio buffer layout for the audio ports.
pub struct RawBufferLayout<'a> {
    /// The `f32` audio buffers for each audio input port, in order.
    ///
    /// A buffer can be `None` because of any of these conditions:
    ///
    /// * This input port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, and the host has decided to give this port 64 bit
    /// buffers. In that case the buffer will exist in `in_f64` instead.
    /// * This input port belongs to an "in_place_pair" specified in
    /// `AudioPortInfo::in_place_pair_id` (if this plugin does not use the
    /// `PluginAudioPortsExtension` then it is by default), and the host
    /// has decided to give the same buffer for that input/output pair. In
    /// this case, that shared buffer will live in either `out_f32` or `out_f64`.
    pub in_f32: &'a [Option<AudioPortBuffer<f32>>],

    /// The `f64` audio buffers for each audio input port, in order.
    ///
    /// A buffer can be `None` because of any of these conditions:
    ///
    /// * This input port has not requested to use 64 bit buffers in
    /// `AudioPortInfo::flags` (if this plugin does not use the
    /// `PluginAudioPortsExtension` then it does not request 64 bit buffers
    /// by default). In this case this will always be `None`.
    /// * This input port *has* requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, but the host has decided to give this port 32
    /// bit buffers anyway. In that case the buffer will exist in `in_f32`
    /// instead.
    /// * This input port belongs to an "in_place_pair" specified in
    /// `AudioPortInfo::in_place_pair_id` (if this plugin does not use the
    /// `PluginAudioPortsExtension` then it is by default), and the host
    /// has decided to give the same buffer for that input/output pair. In
    /// this case, that shared buffer will live in either `out_f32` or `out_f64`.
    pub in_f64: &'a [Option<AudioPortBuffer<f64>>],

    /// The `f32` audio buffers for each audio output port, in order.
    ///
    /// A buffer can be `None` because in this condition:
    ///
    /// * This output port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, and the host has decided to give this port 64 bit
    /// buffers. In that case the buffer will exist in `out_f64` instead.
    ///
    /// # SAFETY
    ///
    /// Undefined behavior may occur if you change any `None` to `Some` or
    /// vice versa. So please don't do that.
    pub out_f32: &'a mut [Option<AudioPortBuffer<f32>>],

    /// The `f64` audio buffers for each audio output port, in order.
    ///
    /// A buffer can be `None` because of any of these conditions:
    ///
    /// * This output port has not requested to use 64 bit buffers in
    /// `AudioPortInfo::flags` (if this plugin does not use the
    /// `PluginAudioPortsExtension` then it does not request 64 bit buffers
    /// by default). In this case this will always be `None`.
    /// * This output port *has* requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, but the host has decided to give this port 32
    /// bit buffers anyway. In that case the buffer will exist in `out_f32`
    /// instead.
    ///
    /// # SAFETY
    ///
    /// Undefined behavior may occur if you change any `None` to `Some` or
    /// vice versa. So please don't do that.
    pub out_f64: &'a mut [Option<AudioPortBuffer<f64>>],
}
