#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurrentMainLayout {
    StereoInPlace32,
    StereoInPlace64,

    StereoInOut32,
    StereoInOut64,

    StereoIn32,
    StereoIn64,

    StereoOut32,
    StereoOut64,

    MonoInPlace32,
    MonoInPlace64,

    MonoInOut32,
    MonoInOut64,

    MonoIn32,
    MonoIn64,

    MonoOut32,
    MonoOut64,

    MonoInStereoOut32,
    MonoInStereoOut64,

    StereoInMonoOut32,
    StereoInMonoOut64,

    Custom,

    None,
}

pub enum StereoIn64Res<'a> {
    F64 { left: &'a [f64], right: &'a [f64] },
    F32 { left: &'a [f32], right: &'a [f32] },
}

pub enum StereoOut64Res<'a> {
    F64 { left: &'a mut [f64], right: &'a mut [f64] },
    F32 { left: &'a mut [f32], right: &'a mut [f32] },
}

pub enum MonoIn64Res<'a> {
    F64(&'a [f64]),
    F32(&'a [f32]),
}

pub enum MonoOut64Res<'a> {
    F64(&'a mut [f64]),
    F32(&'a mut [f32]),
}

pub enum StereoInPlace32Res<'a> {
    InPlace {
        left: &'a mut [f32],
        right: &'a mut [f32],
    },
    Separate {
        in_left: &'a [f32],
        in_right: &'a [f32],

        out_left: &'a mut [f32],
        out_right: &'a mut [f32],
    },
}

pub enum StereoInPlace64Res<'a> {
    InPlace64 {
        left: &'a mut [f64],
        right: &'a mut [f64],
    },
    Separate64 {
        in_left: &'a [f64],
        in_right: &'a [f64],

        out_left: &'a mut [f64],
        out_right: &'a mut [f64],
    },
    InPlace32 {
        left: &'a mut [f32],
        right: &'a mut [f32],
    },
    Separate32 {
        in_left: &'a [f32],
        in_right: &'a [f32],

        out_left: &'a mut [f32],
        out_right: &'a mut [f32],
    },
}

pub enum StereoInOut64Res<'a> {
    F64 {
        in_left: &'a [f64],
        in_right: &'a [f64],

        out_left: &'a mut [f64],
        out_right: &'a mut [f64],
    },
    F32 {
        in_left: &'a [f32],
        in_right: &'a [f32],

        out_left: &'a mut [f32],
        out_right: &'a mut [f32],
    },
}

pub enum MonoInPlace32Res<'a> {
    InPlace(&'a mut [f32]),
    Separate { input: &'a [f32], output: &'a mut [f32] },
}

pub enum MonoInPlace64Res<'a> {
    InPlace64(&'a mut [f64]),
    Separate64 { input: &'a [f64], output: &'a mut [f64] },
    InPlace32(&'a mut [f32]),
    Separate32 { input: &'a [f32], output: &'a mut [f32] },
}

pub enum MonoInOut64Res<'a> {
    F64 { input: &'a [f64], output: &'a mut [f64] },
    F32 { input: &'a [f32], output: &'a mut [f32] },
}

pub enum MonoInStereOut64Res<'a> {
    F64 { input: &'a [f64], out_left: &'a mut [f64], out_right: &'a mut [f64] },
    F32 { input: &'a [f32], out_left: &'a mut [f32], out_right: &'a mut [f32] },
}

pub enum StereoInMonoOut64Res<'a> {
    F64 { in_left: &'a [f64], in_right: &'a [f64], output: &'a mut [f64] },
    F32 { in_left: &'a [f32], in_right: &'a [f32], output: &'a mut [f32] },
}
