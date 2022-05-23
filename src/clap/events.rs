use clap_sys::events::clap_event_header as RawClapEventHeader;
use clap_sys::events::clap_input_events as RawClapInputEvents;
use clap_sys::events::clap_output_events as RawClapOutputEvents;

pub struct ClapInputEvents {
    raw: RawClapInputEvents,
}

impl ClapInputEvents {
    pub fn new() -> Self {
        let raw = RawClapInputEvents { ctx: std::ptr::null_mut(), size, get };

        Self { raw }
    }

    pub fn raw(&self) -> *const RawClapInputEvents {
        &self.raw
    }
}

pub struct ClapOutputEvents {
    raw: RawClapOutputEvents,
}

impl ClapOutputEvents {
    pub fn new() -> Self {
        let raw = RawClapOutputEvents { ctx: std::ptr::null_mut(), try_push };

        Self { raw }
    }

    pub fn raw(&self) -> *const RawClapOutputEvents {
        &self.raw
    }
}

unsafe extern "C" fn size(list: *const RawClapInputEvents) -> u32 {
    // TODO
    0
}

unsafe extern "C" fn get(list: *const RawClapInputEvents, index: u32) -> *const RawClapEventHeader {
    // TODO
    std::ptr::null()
}

unsafe extern "C" fn try_push(
    list: *const RawClapOutputEvents,
    event: *const RawClapEventHeader,
) -> bool {
    // TODO
    false
}
