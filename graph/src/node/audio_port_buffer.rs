pub enum AudioPortBuffer<'a> {
    F32(AudioPortBufferF32<'a>),
    F64(AudioPortBufferF64<'a>),
}

pub struct AudioPortBufferF32<'a> {
    pub channels: &'a [&'a f32],
    pub constant_mask: u64,
}

pub struct AudioPortBufferF64<'a> {
    pub channels: &'a [&'a f64],
    pub constant_mask: u64,
}

pub enum AudioPortBufferMut<'a> {
    F32(AudioPortBufferF32Mut<'a>),
    F64(AudioPortBufferF64Mut<'a>),
}

pub struct AudioPortBufferF32Mut<'a> {
    pub channels: &'a [&'a mut f32],
    pub constant_mask: u64,
}

pub struct AudioPortBufferF64Mut<'a> {
    pub channels: &'a [&'a mut f64],
    pub constant_mask: u64,
}
