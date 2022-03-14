use std::fmt::DebugStruct;

use super::buffer_layout::CurrentBufferLayout;
use super::{AudioPortBuffer, ProcBufferLayout, ProcInfo, RawBufferLayout};

/// The audio port buffers sent to the plugin's `process()` method.
pub struct ProcAudioBuffers {
    in_f32: Vec<Option<AudioPortBuffer<f32>>>,
    in_f64: Vec<Option<AudioPortBuffer<f64>>>,

    out_f32: Vec<Option<AudioPortBuffer<f32>>>,
    out_f64: Vec<Option<AudioPortBuffer<f64>>>,

    layout: CurrentBufferLayout,
}

impl ProcAudioBuffers {
    pub(crate) fn debug_fields(&self, f: &mut DebugStruct) {
        f.field("audio_layout: {:?}", &self.layout);
        f.field("in_f32: {}", &self.in_f32);
        f.field("in_f64: {}", &self.in_f64);
        f.field("out_f64: {}", &self.out_f32);
        f.field("out_f64: {}", &self.out_f64);
    }

    /// Get the layout of audio buffers.
    ///
    /// If the plugin is using the the default port layout of
    /// `AudioPortLayout::StereoInPlace`, then the host will always return one of
    /// these options in this method:
    ///
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    ///
    /// If the plugin is using a different port layout then the default, then
    /// refer to the documentation in [`AudioPortLayout`] to know what options
    /// the host may return in this method.
    ///
    /// [`AudioPortLayout`]: ../../plugin/ext/audio_ports/enum.AudioPortLayout.html
    pub fn get<'a>(&'a mut self, proc_info: &ProcInfo) -> ProcBufferLayout<'a> {
        // Safe because the scheduler ensures that the correct buffers exist
        // for `self.layout`.
        unsafe {
            match self.layout {
                CurrentBufferLayout::StereoOut32 => {
                    let (left, right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoOut32 { left, right }
                }
                CurrentBufferLayout::StereoOut64 => {
                    let (left, right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoOut64 { left, right }
                }

                CurrentBufferLayout::MonoOut32 => {
                    let b = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoOut32(b)
                }
                CurrentBufferLayout::MonoOut64 => {
                    let b = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoOut64(b)
                }

                CurrentBufferLayout::StereoInPlace32 => {
                    let (left, right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInPlace32 { left, right }
                }
                CurrentBufferLayout::StereoInPlace64 => {
                    let (left, right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInPlace64 { left, right }
                }

                CurrentBufferLayout::StereoInPlaceWithSidechain32 => {
                    let (left, right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    let (sc_left, sc_right) = self
                        .in_f32
                        .get_unchecked(1)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    ProcBufferLayout::StereoInPlaceWithSidechain32 {
                        left,
                        right,
                        sc_left,
                        sc_right,
                    }
                }
                CurrentBufferLayout::StereoInPlaceWithSidechain64 => {
                    let (left, right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    let (sc_left, sc_right) = self
                        .in_f64
                        .get_unchecked(1)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    ProcBufferLayout::StereoInPlaceWithSidechain64 {
                        left,
                        right,
                        sc_left,
                        sc_right,
                    }
                }

                CurrentBufferLayout::StereoInPlaceWithExtraOut32 => {
                    let (out_1, out_2) = self.out_f32.split_first_mut().unwrap_unchecked();
                    let (left, right) =
                        out_1.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info);
                    let (extra_out_left, extra_out_right) = out_2
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInPlaceWithExtraOut32 {
                        left,
                        right,
                        extra_out_left,
                        extra_out_right,
                    }
                }
                CurrentBufferLayout::StereoInPlaceWithExtraOut64 => {
                    let (out_1, out_2) = self.out_f64.split_first_mut().unwrap_unchecked();
                    let (left, right) =
                        out_1.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info);
                    let (extra_out_left, extra_out_right) = out_2
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInPlaceWithExtraOut64 {
                        left,
                        right,
                        extra_out_left,
                        extra_out_right,
                    }
                }

                CurrentBufferLayout::MonoInPlace32 => {
                    let b = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoInPlace32(b)
                }
                CurrentBufferLayout::MonoInPlace64 => {
                    let b = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoInPlace64(b)
                }

                CurrentBufferLayout::MonoInPlaceWithSidechain32 => {
                    let in_out = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    let sc =
                        self.in_f32.get_unchecked(1).as_ref().unwrap_unchecked().mono(proc_info);
                    ProcBufferLayout::MonoInPlaceWithSidechain32 { in_out, sc }
                }
                CurrentBufferLayout::MonoInPlaceWithSidechain64 => {
                    let in_out = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    let sc =
                        self.in_f64.get_unchecked(1).as_ref().unwrap_unchecked().mono(proc_info);
                    ProcBufferLayout::MonoInPlaceWithSidechain64 { in_out, sc }
                }

                CurrentBufferLayout::StereoInOut32 => {
                    let (in_left, in_right) = self
                        .in_f32
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_left, out_right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInOut32 { in_left, in_right, out_left, out_right }
                }
                CurrentBufferLayout::StereoInOut64 => {
                    let (in_left, in_right) = self
                        .in_f64
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_left, out_right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInOut64 { in_left, in_right, out_left, out_right }
                }

                CurrentBufferLayout::StereoInOutWithSidechain32 => {
                    let (in_left, in_right) = self
                        .in_f32
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_left, out_right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    let (sc_left, sc_right) = self
                        .in_f32
                        .get_unchecked(1)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    ProcBufferLayout::StereoInOutWithSidechain32 {
                        in_left,
                        in_right,
                        out_left,
                        out_right,
                        sc_left,
                        sc_right,
                    }
                }
                CurrentBufferLayout::StereoInOutWithSidechain64 => {
                    let (in_left, in_right) = self
                        .in_f64
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_left, out_right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    let (sc_left, sc_right) = self
                        .in_f64
                        .get_unchecked(1)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    ProcBufferLayout::StereoInOutWithSidechain64 {
                        in_left,
                        in_right,
                        out_left,
                        out_right,
                        sc_left,
                        sc_right,
                    }
                }

                CurrentBufferLayout::StereoInOutWithExtraOut32 => {
                    let (in_left, in_right) = self
                        .in_f32
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_1, out_2) = self.out_f32.split_first_mut().unwrap_unchecked();
                    let (out_left, out_right) =
                        out_1.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info);
                    let (extra_out_left, extra_out_right) = out_2
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInOutWithExtraOut32 {
                        in_left,
                        in_right,
                        out_left,
                        out_right,
                        extra_out_left,
                        extra_out_right,
                    }
                }
                CurrentBufferLayout::StereoInOutWithExtraOut64 => {
                    let (in_left, in_right) = self
                        .in_f64
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let (out_1, out_2) = self.out_f64.split_first_mut().unwrap_unchecked();
                    let (out_left, out_right) =
                        out_1.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info);
                    let (extra_out_left, extra_out_right) = out_2
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::StereoInOutWithExtraOut64 {
                        in_left,
                        in_right,
                        out_left,
                        out_right,
                        extra_out_left,
                        extra_out_right,
                    }
                }

                CurrentBufferLayout::MonoInOut32 => {
                    let input =
                        self.in_f32.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let output = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoInOut32 { input, output }
                }
                CurrentBufferLayout::MonoInOut64 => {
                    let input =
                        self.in_f64.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let output = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::MonoInOut64 { input, output }
                }

                CurrentBufferLayout::MonoInOutWithSidechain32 => {
                    let input =
                        self.in_f32.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let output = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    let sc =
                        self.in_f32.get_unchecked(1).as_ref().unwrap_unchecked().mono(proc_info);
                    ProcBufferLayout::MonoInOutWithSidechain32 { input, output, sc }
                }
                CurrentBufferLayout::MonoInOutWithSidechain64 => {
                    let input =
                        self.in_f64.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let output = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    let sc =
                        self.in_f64.get_unchecked(1).as_ref().unwrap_unchecked().mono(proc_info);
                    ProcBufferLayout::MonoInOutWithSidechain64 { input, output, sc }
                }

                CurrentBufferLayout::MonoInStereoOut32 => {
                    let input =
                        self.in_f32.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let (out_left, out_right) = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::MonoInStereoOut32 { input, out_left, out_right }
                }
                CurrentBufferLayout::MonoInStereoOut64 => {
                    let input =
                        self.in_f64.get_unchecked(0).as_ref().unwrap_unchecked().mono(proc_info);
                    let (out_left, out_right) = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .stereo_unchecked_mut(proc_info);
                    ProcBufferLayout::MonoInStereoOut64 { input, out_left, out_right }
                }

                CurrentBufferLayout::StereoInMonoOut32 => {
                    let (in_left, in_right) = self
                        .in_f32
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let output = self
                        .out_f32
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::StereoInMonoOut32 { in_left, in_right, output }
                }
                CurrentBufferLayout::StereoInMonoOut64 => {
                    let (in_left, in_right) = self
                        .in_f64
                        .get_unchecked(0)
                        .as_ref()
                        .unwrap_unchecked()
                        .stereo_unchecked(proc_info);
                    let output = self
                        .out_f64
                        .get_unchecked_mut(0)
                        .as_mut()
                        .unwrap_unchecked()
                        .mono_mut(proc_info);
                    ProcBufferLayout::StereoInMonoOut64 { in_left, in_right, output }
                }

                /* TODO
                CurrentBufferLayout::SurroundOut32 => {
                    let b = self.out_f32.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundOut32(b)
                }
                CurrentBufferLayout::SurroundOut64 => {
                    let b = self.out_f64.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundOut64(b)
                }

                CurrentBufferLayout::SurroundInPlace32 => {
                    let b = self.out_f32.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundInPlace32(b)
                }
                CurrentBufferLayout::SurroundInPlace64 => {
                    let b = self.out_f64.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundInPlace64(b)
                }

                CurrentBufferLayout::SurroundInPlaceWithSidechain32 => {
                    let in_out = self.out_f32.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    let sc = self.in_f32.get_unchecked(1).as_ref().unwrap_unchecked();
                    ProcBufferLayout::SurroundInPlaceWithSidechain32 { in_out, sc }
                }
                CurrentBufferLayout::SurroundInPlaceWithSidechain64 => {
                    let in_out = self.out_f64.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    let sc = self.in_f64.get_unchecked(1).as_ref().unwrap_unchecked();
                    ProcBufferLayout::SurroundInPlaceWithSidechain64 { in_out, sc }
                }

                CurrentBufferLayout::SurroundInOut32 => {
                    let input = self.in_f32.get_unchecked(0).as_ref().unwrap_unchecked();
                    let output = self.out_f32.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundInOut32 { input, output }
                }
                CurrentBufferLayout::SurroundInOut64 => {
                    let input = self.in_f64.get_unchecked(0).as_ref().unwrap_unchecked();
                    let output = self.out_f64.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    ProcBufferLayout::SurroundInOut64 { input, output }
                }

                CurrentBufferLayout::SurroundInOutWithSidechain32 => {
                    let input = self.in_f32.get_unchecked(0).as_ref().unwrap_unchecked();
                    let output = self.out_f32.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    let sc = self.in_f32.get_unchecked(1).as_ref().unwrap_unchecked();
                    ProcBufferLayout::SurroundInOutWithSidechai32 { input, output, sc }
                }
                CurrentBufferLayout::SurroundInOutWithSidechain64 => {
                    let input = self.in_f64.get_unchecked(0).as_ref().unwrap_unchecked();
                    let output = self.out_f64.get_unchecked_mut(0).as_mut().unwrap_unchecked();
                    let sc = self.in_f64.get_unchecked(1).as_ref().unwrap_unchecked();
                    ProcBufferLayout::SurroundInOutWithSidechain64 { input, output, sc }
                }
                */
                CurrentBufferLayout::Custom => ProcBufferLayout::Custom(RawBufferLayout {
                    in_f32: &self.in_f32,
                    in_f64: &self.in_f64,

                    out_f32: &mut self.out_f32,
                    out_f64: &mut self.out_f64,
                }),
            }
        }
    }

    /// Get the raw layout of audio buffers.
    #[inline]
    pub fn raw<'a>(&'a mut self) -> RawBufferLayout<'a> {
        RawBufferLayout {
            in_f32: &self.in_f32,
            in_f64: &self.in_f64,

            out_f32: &mut self.out_f32,
            out_f64: &mut self.out_f64,
        }
    }

    /// NOT IMPLEMENTED YET
    ///
    /// Returns `true` if the audio buffers for this input port
    /// are silent (all zeros), `false` otherwise.
    pub fn in_port_is_silent(&self, port_index: usize) -> bool {
        false
    }

    /// Clear all of the output buffers to `0.0`.
    pub fn clear_all_outputs(&mut self, proc_info: &ProcInfo) {
        for b in self.out_f32.iter_mut() {
            if let Some(b) = b.as_mut() {
                b.clear(proc_info);
            }
        }
        for b in self.out_f64.iter_mut() {
            if let Some(b) = b.as_mut() {
                b.clear(proc_info);
            }
        }
    }

    pub(crate) fn assert_layout(&self) -> Result<(), ()> {
        match self.layout {
            CurrentBufferLayout::StereoOut32 => {
                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoOut64 => {
                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoOut32 => {
                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoOut64 => {
                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInPlace32 => {
                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInPlace64 => {
                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInPlaceWithSidechain32 => {
                if self.in_f32.len() < 2 {
                    return Err(());
                }
                if self.in_f32[1].is_none() {
                    return Err(());
                }
                if self.in_f32[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInPlaceWithSidechain64 => {
                if self.in_f64.len() < 2 {
                    return Err(());
                }
                if self.in_f64[1].is_none() {
                    return Err(());
                }
                if self.in_f64[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInPlaceWithExtraOut32 => {
                if self.out_f32.len() < 2 {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32[1].is_none() {
                    return Err(());
                }
                if self.out_f32[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInPlaceWithExtraOut64 => {
                if self.out_f64.len() < 2 {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64[1].as_ref().is_none() {
                    return Err(());
                }
                if self.out_f64[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoInPlace32 => {
                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoInPlace64 => {
                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoInPlaceWithSidechain32 => {
                if self.in_f32.len() < 2 {
                    return Err(());
                }
                if self.in_f32[1].is_none() {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoInPlaceWithSidechain64 => {
                if self.in_f64.len() < 2 {
                    return Err(());
                }
                if self.in_f64[1].is_none() {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInOut32 => {
                if self.in_f32.is_empty() {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }
                if self.in_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInOut64 => {
                if self.in_f64.is_empty() {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }
                if self.in_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInOutWithSidechain32 => {
                if self.in_f32.len() < 2 {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }
                if self.in_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.in_f32[1].is_none() {
                    return Err(());
                }
                if self.in_f32[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInOutWithSidechain64 => {
                if self.in_f64.len() < 2 {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }
                if self.in_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.in_f64[1].is_none() {
                    return Err(());
                }
                if self.in_f64[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInOutWithExtraOut32 => {
                if self.in_f32.is_empty() {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }
                if self.in_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32.len() < 2 {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32[1].is_none() {
                    return Err(());
                }
                if self.out_f32[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInOutWithExtraOut64 => {
                if self.in_f64.is_empty() {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }
                if self.in_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64.len() < 2 {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64[1].is_none() {
                    return Err(());
                }
                if self.out_f64[1].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoInOut32 => {
                if self.in_f32.is_empty() {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoInOut64 => {
                if self.in_f64.is_empty() {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoInOutWithSidechain32 => {
                if self.in_f32.len() < 2 {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }

                if self.in_f32[1].is_none() {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoInOutWithSidechain64 => {
                if self.in_f64.len() < 2 {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }

                if self.in_f64[1].is_none() {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::MonoInStereoOut32 => {
                if self.in_f32.is_empty() {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
                if self.out_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }
            CurrentBufferLayout::MonoInStereoOut64 => {
                if self.in_f64.is_empty() {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
                if self.out_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }
            }

            CurrentBufferLayout::StereoInMonoOut32 => {
                if self.in_f32.is_empty() {
                    return Err(());
                }
                if self.in_f32[0].is_none() {
                    return Err(());
                }
                if self.in_f32[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f32.is_empty() {
                    return Err(());
                }
                if self.out_f32[0].is_none() {
                    return Err(());
                }
            }
            CurrentBufferLayout::StereoInMonoOut64 => {
                if self.in_f64.is_empty() {
                    return Err(());
                }
                if self.in_f64[0].is_none() {
                    return Err(());
                }
                if self.in_f64[0].as_ref().unwrap().channel_count() < 2 {
                    return Err(());
                }

                if self.out_f64.is_empty() {
                    return Err(());
                }
                if self.out_f64[0].is_none() {
                    return Err(());
                }
            }

            CurrentBufferLayout::Custom => {}
        }

        Ok(())
    }
}
