use std::fmt::DebugStruct;

use super::buffer_layout::{
    CurrentMainLayout, MonoIn64Res, MonoInOut64Res, MonoInPlace32Res, MonoInPlace64Res,
    MonoInStereOut64Res, MonoOut64Res, StereoIn64Res, StereoInMonoOut64Res, StereoInOut64Res,
    StereoInPlace32Res, StereoInPlace64Res, StereoOut64Res,
};
use super::{AudioPortBuffer, ProcInfo};

/// The audio port buffers sent to the plugin's `process()` method.
pub struct ProcAudioBuffers {
    /// The audio buffers for the main audio ports.
    pub main: ProcMainBuffers,

    /// The audio buffers for any extra audio ports (**NOT** including the
    /// main ports).
    pub extra: ProcExtraBuffers,
}

pub struct ProcMainBuffers {
    in_f32: Option<AudioPortBuffer<f32>>,
    in_f64: Option<AudioPortBuffer<f64>>,
    out_f32: Option<AudioPortBuffer<f32>>,
    out_f64: Option<AudioPortBuffer<f64>>,

    layout: CurrentMainLayout,
}

impl ProcMainBuffers {
    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoIn`. This will always return `None`
    /// otherwise.
    pub fn stereo_in(&self, proc_info: &ProcInfo) -> Option<(&[f32], &[f32])> {
        if let CurrentMainLayout::StereoIn32 = self.layout {
            let (left, right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            Some((left, right))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInPrefers64`. This will always return `None`
    /// otherwise.
    pub fn stereo_in_prefers_64<'a>(&'a self, proc_info: &ProcInfo) -> Option<StereoIn64Res<'a>> {
        if let CurrentMainLayout::StereoIn64 = self.layout {
            let (left, right) =
                unsafe { self.in_f64.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            Some(StereoIn64Res::F64 { left, right })
        } else if let CurrentMainLayout::StereoIn32 = self.layout {
            let (left, right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            Some(StereoIn64Res::F32 { left, right })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoOut`. This will always return `None`
    /// otherwise.
    pub fn stereo_out(&mut self, proc_info: &ProcInfo) -> Option<(&mut [f32], &mut [f32])> {
        if let CurrentMainLayout::StereoOut32 = self.layout {
            let (left, right) =
                unsafe { self.in_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some((left, right))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn stereo_out_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<StereoOut64Res<'a>> {
        if let CurrentMainLayout::StereoOut64 = self.layout {
            let (left, right) =
                unsafe { self.in_f64.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoOut64Res::F64 { left, right })
        } else if let CurrentMainLayout::StereoOut32 = self.layout {
            let (left, right) =
                unsafe { self.in_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoOut64Res::F32 { left, right })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoIn`. This will always return `None`
    /// otherwise.
    pub fn mono_in(&self, proc_info: &ProcInfo) -> Option<&[f32]> {
        if let CurrentMainLayout::MonoIn32 = self.layout {
            let b = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            Some(b)
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInPrefers64`. This will always return `None`
    /// otherwise.
    pub fn mono_in_prefers_64<'a>(&'a self, proc_info: &ProcInfo) -> Option<MonoIn64Res<'a>> {
        if let CurrentMainLayout::MonoIn64 = self.layout {
            let b = unsafe { self.in_f64.as_ref().unwrap_unchecked().mono(proc_info) };
            Some(MonoIn64Res::F64(b))
        } else if let CurrentMainLayout::MonoIn32 = self.layout {
            let b = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            Some(MonoIn64Res::F32(b))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoOut`. This will always return `None`
    /// otherwise.
    pub fn mono_out(&mut self, proc_info: &ProcInfo) -> Option<&mut [f32]> {
        if let CurrentMainLayout::MonoOut32 = self.layout {
            let b = unsafe { self.in_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(b)
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn mono_out_prefers_64<'a>(&'a mut self, proc_info: &ProcInfo) -> Option<MonoOut64Res<'a>> {
        if let CurrentMainLayout::MonoOut64 = self.layout {
            let b = unsafe { self.in_f64.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoOut64Res::F64(b))
        } else if let CurrentMainLayout::MonoOut32 = self.layout {
            let b = unsafe { self.in_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoOut64Res::F32(b))
        } else {
            None
        }
    }

    /// If the plugin uses the default for the `AudioPortsExtension`, then this will
    /// always return `Some`.
    ///
    /// If the plugin does not use the default for the `AudioPortsExtension` and it
    /// does not use `MainPortsLayout::StereoInPlace`, then this will return `None`.
    pub fn stereo_in_place<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<StereoInPlace32Res<'a>> {
        if let CurrentMainLayout::StereoInPlace32 = self.layout {
            let (left, right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace32Res::InPlace { left, right })
        } else if let CurrentMainLayout::StereoInOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace32Res::Separate { in_left, in_right, out_left, out_right })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInPlacePrefersF64`. This will always return
    /// `None` otherwise.
    pub fn stereo_in_place_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<StereoInPlace64Res<'a>> {
        if let CurrentMainLayout::StereoInPlace64 = self.layout {
            let (left, right) =
                unsafe { self.out_f64.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace64Res::InPlace64 { left, right })
        } else if let CurrentMainLayout::StereoInOut64 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f64.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f64.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace64Res::Separate64 { in_left, in_right, out_left, out_right })
        } else if let CurrentMainLayout::StereoInPlace32 = self.layout {
            let (left, right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace64Res::InPlace32 { left, right })
        } else if let CurrentMainLayout::StereoInOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInPlace64Res::Separate32 { in_left, in_right, out_left, out_right })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInOut`. This will always return `None`
    /// otherwise.
    pub fn stereo_in_out(
        &mut self,
        proc_info: &ProcInfo,
    ) -> Option<(&[f32], &[f32], &mut [f32], &mut [f32])> {
        if let CurrentMainLayout::StereoInOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some((in_left, in_right, out_left, out_right))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn stereo_in_out_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<StereoInOut64Res<'a>> {
        if let CurrentMainLayout::StereoInOut64 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f64.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f64.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInOut64Res::F64 { in_left, in_right, out_left, out_right })
        } else if let CurrentMainLayout::StereoInOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(StereoInOut64Res::F32 { in_left, in_right, out_left, out_right })
        } else {
            None
        }
    }

    /// If the plugin uses the default for the `AudioPortsExtension`, then this will
    /// always return `Some`.
    ///
    /// If the plugin does not use the default for the `AudioPortsExtension` and it
    /// does not use `MainPortsLayout::MonoInPlace`, then this will return `None`.
    pub fn mono_in_place<'a>(&'a mut self, proc_info: &ProcInfo) -> Option<MonoInPlace32Res<'a>> {
        if let CurrentMainLayout::MonoInPlace32 = self.layout {
            let b = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace32Res::InPlace(b))
        } else if let CurrentMainLayout::MonoInOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace32Res::Separate { input, output })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInPlacePrefersF64`. This will always return
    /// `None` otherwise.
    pub fn mono_in_place_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<MonoInPlace64Res<'a>> {
        if let CurrentMainLayout::MonoInPlace64 = self.layout {
            let b = unsafe { self.out_f64.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace64Res::InPlace64(b))
        } else if let CurrentMainLayout::MonoInOut64 = self.layout {
            let input = unsafe { self.in_f64.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f64.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace64Res::Separate64 { input, output })
        } else if let CurrentMainLayout::MonoInPlace32 = self.layout {
            let b = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace64Res::InPlace32(b))
        } else if let CurrentMainLayout::MonoInOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInPlace64Res::Separate32 { input, output })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInOut`. This will always return `None`
    /// otherwise.
    pub fn mono_in_out(&mut self, proc_info: &ProcInfo) -> Option<(&[f32], &mut [f32])> {
        if let CurrentMainLayout::MonoInOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some((input, output))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn mono_in_out_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<MonoInOut64Res<'a>> {
        if let CurrentMainLayout::MonoInOut64 = self.layout {
            let input = unsafe { self.in_f64.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f64.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInOut64Res::F64 { input, output })
        } else if let CurrentMainLayout::MonoInOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(MonoInOut64Res::F32 { input, output })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInStereoOut`. This will always return `None`
    /// otherwise.
    pub fn mono_in_stereo_out(
        &mut self,
        proc_info: &ProcInfo,
    ) -> Option<(&[f32], &mut [f32], &mut [f32])> {
        if let CurrentMainLayout::MonoInStereoOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some((input, out_left, out_right))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::MonoInStereoOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn mono_in_stereo_out_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<MonoInStereOut64Res<'a>> {
        if let CurrentMainLayout::MonoInStereoOut64 = self.layout {
            let input = unsafe { self.in_f64.as_ref().unwrap_unchecked().mono(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f64.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(MonoInStereOut64Res::F64 { input, out_left, out_right })
        } else if let CurrentMainLayout::MonoInStereoOut32 = self.layout {
            let input = unsafe { self.in_f32.as_ref().unwrap_unchecked().mono(proc_info) };
            let (out_left, out_right) =
                unsafe { self.out_f32.as_mut().unwrap_unchecked().stereo_unchecked_mut(proc_info) };
            Some(MonoInStereOut64Res::F32 { input, out_left, out_right })
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInMonoOut`. This will always return `None`
    /// otherwise.
    pub fn stereo_in_mono_out(
        &mut self,
        proc_info: &ProcInfo,
    ) -> Option<(&[f32], &[f32], &mut [f32])> {
        if let CurrentMainLayout::StereoInMonoOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some((in_left, in_right, output))
        } else {
            None
        }
    }

    /// This will always return `Some` if the plugin uses the `AudioPortsExtension`
    /// and uses `MainPortsLayout::StereoInMonoOutPrefers64`. This will always return `None`
    /// otherwise.
    pub fn stereo_in_mono_out_prefers_64<'a>(
        &'a mut self,
        proc_info: &ProcInfo,
    ) -> Option<StereoInMonoOut64Res> {
        if let CurrentMainLayout::StereoInMonoOut64 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f64.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let output = unsafe { self.out_f64.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(StereoInMonoOut64Res::F64 { in_left, in_right, output })
        } else if let CurrentMainLayout::StereoInMonoOut32 = self.layout {
            let (in_left, in_right) =
                unsafe { self.in_f32.as_ref().unwrap_unchecked().stereo_unchecked(proc_info) };
            let output = unsafe { self.out_f32.as_mut().unwrap_unchecked().mono_mut(proc_info) };
            Some(StereoInMonoOut64Res::F32 { in_left, in_right, output })
        } else {
            None
        }
    }

    /// Get the raw buffers.
    ///
    /// Use this if the plugin uses the `AudioPortsExtension` and uses
    /// `MainPortsLayout::Custom`.
    pub fn raw<'a>(&'a mut self) -> RawMainBuffers<'a> {
        RawMainBuffers {
            in_f32: &self.in_f32,
            in_f64: &self.in_f64,
            out_f32: &mut self.out_f32,
            out_f64: &mut self.out_f64,
        }
    }

    /// NOT IMPLEMENTED YET
    ///
    /// Returns `true` if the main audio input buffer exists and is silent (all zeros),
    /// false otherwise.
    pub fn in_port_is_silent(&self) -> bool {
        false
    }
}

pub struct ProcExtraBuffers {
    in_f32: Vec<Option<AudioPortBuffer<f32>>>,
    in_f64: Vec<Option<AudioPortBuffer<f64>>>,
    out_f32: Vec<Option<AudioPortBuffer<f32>>>,
    out_f64: Vec<Option<AudioPortBuffer<f64>>>,
}

impl ProcExtraBuffers {
    pub fn raw<'a>(&'a mut self) -> RawExtraBuffers<'a> {
        RawExtraBuffers {
            in_f32: &self.in_f32,
            in_f64: &self.in_f64,
            out_f32: &mut self.out_f32,
            out_f64: &mut self.out_f64,
        }
    }

    /// NOT IMPLEMENTED YET
    ///
    /// Returns `true` if the extra audio input buffer exists and is silent (all zeros),
    /// false otherwise.
    pub fn in_port_is_silent(&self, port_index: usize) -> bool {
        false
    }
}

/// The raw audio buffers for the main audio ports.
pub struct RawMainBuffers<'a> {
    /// The `f32` audio buffer for the main audio input port.
    ///
    /// This can be `None` because of any of these conditions:
    ///
    /// * This plugin has no main input port.
    /// * This input port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, and the host has decided to give this port 64 bit
    /// buffers. In that case the buffer will exist in `in_f64` instead.
    /// * This input port is in an "in_place_pair" with the main output port, and
    /// the host has decided to give a single buffer for both the main input and
    /// output ports. In this case the buffer will exist in `out_f32` or `out_f64`
    /// instead.
    pub in_f32: &'a Option<AudioPortBuffer<f32>>,

    /// The `f64` audio buffer for the main audio input port.
    ///
    /// This can be `None` because of any of these conditions:
    ///
    /// * This plugin has no main input port.
    /// * This input port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, but the host has decided to give this port 32 bit
    /// buffers. In that case the buffer will exist in `in_f32` instead.
    /// * This input port is in an "in_place_pair" with the main output port, and
    /// the host has decided to give a single buffer for both the main input and
    /// output ports. In this case the buffer will exist in `out_f32` or `out_f64`
    /// instead.
    pub in_f64: &'a Option<AudioPortBuffer<f64>>,

    /// The `f32` audio buffer for the main audio output port.
    ///
    /// This can be `None` because of any of these conditions:
    ///
    /// * This plugin has no main output port.
    /// * This output port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, and the host has decided to give this port 64 bit
    /// buffers. In that case the buffer will exist in `out_f64` instead.
    ///
    /// # SAFETY
    ///
    /// Undefined behavior may occur if you change any `None` to `Some` or
    /// vice versa. So please don't do that.
    pub out_f32: &'a mut Option<AudioPortBuffer<f32>>,

    /// The `f64` audio buffer for the main audio output port.
    ///
    /// This can be `None` because of any of these conditions:
    ///
    /// * This plugin has no main output port.
    /// * This output port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, but the host has decided to give this port 32 bit
    /// buffers. In that case the buffer will exist in `out_f32` instead.
    ///
    /// # SAFETY
    ///
    /// Undefined behavior may occur if you change any `None` to `Some` or
    /// vice versa. So please don't do that.
    pub out_f64: &'a mut Option<AudioPortBuffer<f64>>,
}

/// The raw audio buffers for the extra audio ports (**NOT** including
/// the main ports).
pub struct RawExtraBuffers<'a> {
    /// The `f32` audio buffers for each extra audio input port (**NOT** including
    /// the main port), in order.
    ///
    /// A buffer can be `None` because of any of these conditions:
    ///
    /// * This input port has requested to use 64 bit buffers in
    /// `AudioPortInfo::flags`, and the host has decided to give this port 64 bit
    /// buffers. In that case the buffer will exist in `in_f64` instead.
    pub in_f32: &'a [Option<AudioPortBuffer<f32>>],

    /// The `f64` audio buffers for each extra audio input port (**NOT** including
    /// the main port), in order.
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
    pub in_f64: &'a [Option<AudioPortBuffer<f64>>],

    /// The `f32` audio buffers for each extra audio output port (**NOT** including
    /// the main port), in order.
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

    /// The `f64` audio buffers for each extra audio output port (**NOT** including
    /// the main port), in order.
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

impl ProcAudioBuffers {
    pub(crate) fn debug_fields(&self, f: &mut DebugStruct) {
        f.field("main_layout", &self.main.layout);

        if let Some(main_in_f32) = &self.main.in_f32 {
            f.field("main_in_f32: {:?}", main_in_f32);
        }
        if let Some(main_in_f64) = &self.main.in_f64 {
            f.field("main_in_f64: {:?}", main_in_f64);
        }
        if let Some(main_out_f32) = &self.main.out_f32 {
            f.field("main_out_f32: {:?}", main_out_f32);
        }
        if let Some(main_out_f64) = &self.main.out_f64 {
            f.field("main_out_f64: {:?}", main_out_f64);
        }

        if !self.extra.in_f32.is_empty() {
            f.field("extra_in_f32: {}", &self.extra.in_f32);
        }
        if !self.extra.in_f64.is_empty() {
            f.field("extra_in_f64: {}", &self.extra.in_f64);
        }
        if !self.extra.out_f32.is_empty() {
            f.field("extra_out_f32: {}", &self.extra.out_f32);
        }
        if !self.extra.out_f64.is_empty() {
            f.field("extra_out_f64: {}", &self.extra.out_f64);
        }
    }

    /// Clear all of the output buffers to `0.0`.
    pub fn clear_all_outputs(&mut self, proc_info: &ProcInfo) {
        if let Some(b) = &mut self.main.out_f32 {
            b.clear(proc_info);
        }
        if let Some(b) = &mut self.main.out_f64 {
            b.clear(proc_info);
        }

        for b in self.extra.out_f32.iter_mut() {
            if let Some(b) = b.as_mut() {
                b.clear(proc_info);
            }
        }
        for b in self.extra.out_f64.iter_mut() {
            if let Some(b) = b.as_mut() {
                b.clear(proc_info);
            }
        }
    }
}
