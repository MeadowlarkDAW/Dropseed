use std::ffi::c_void;
use std::pin::Pin;
use std::ptr;

use clap_sys::events::clap_event_header as RawClapEventHeader;
use clap_sys::events::clap_input_events as RawClapInputEvents;
use clap_sys::events::clap_output_events as RawClapOutputEvents;

use crate::plugin::events::event_queue::AllocatedEvent;
use crate::plugin::events::{
    EventMidi, EventMidi2, EventMidiSysex, EventNote, EventNoteExpression, EventParamGesture,
    EventParamMod, EventParamValue, EventTransport,
};
use crate::EventQueue;

pub struct ClapInputEvents {
    raw: Pin<Box<RawClapInputEvents>>,
}

impl ClapInputEvents {
    pub fn new() -> Self {
        Self { raw: Pin::new(Box::new(RawClapInputEvents { ctx: ptr::null_mut(), size, get })) }
    }

    pub fn sync(&mut self, event_queue: *const EventQueue) {
        self.raw.ctx = event_queue as *mut c_void;
    }

    pub fn raw(&self) -> *const RawClapInputEvents {
        &*self.raw
    }
}

pub struct ClapOutputEvents {
    raw: Pin<Box<RawClapOutputEvents>>,
}

impl ClapOutputEvents {
    pub fn new() -> Self {
        Self { raw: Pin::new(Box::new(RawClapOutputEvents { ctx: ptr::null_mut(), try_push })) }
    }

    pub fn sync(&mut self, event_queue: *const EventQueue) {
        self.raw.ctx = event_queue as *mut c_void;
    }

    pub fn raw(&self) -> *const RawClapOutputEvents {
        &*self.raw
    }
}

unsafe fn parse_in_clap_events<'a>(
    clap_events: *const RawClapInputEvents,
) -> Result<&'a EventQueue, ()> {
    if clap_events.is_null() {
        log::warn!("Received a null clap_input_events_t pointer from plugin");
        return Err(());
    }

    let events = &*clap_events;

    if events.ctx.is_null() {
        log::warn!("Received a null clap_input_events_t->ctx pointer from plugin");
        return Err(());
    }

    Ok(&*(events.ctx as *const EventQueue))
}

unsafe fn parse_out_clap_events<'a>(
    clap_events: *const RawClapOutputEvents,
) -> Result<&'a mut EventQueue, ()> {
    if clap_events.is_null() {
        log::warn!("Received a null clap_output_events_t pointer from plugin");
        return Err(());
    }

    let events = &*clap_events;

    if events.ctx.is_null() {
        log::warn!("Received a null clap_output_events_t->ctx pointer from plugin");
        return Err(());
    }

    Ok(&mut *(events.ctx as *const EventQueue as *mut EventQueue))
}

unsafe extern "C" fn size(list: *const RawClapInputEvents) -> u32 {
    let in_events = match parse_in_clap_events(list) {
        Ok(in_events) => in_events,
        Err(()) => return 0,
    };

    in_events.len() as u32
}

unsafe extern "C" fn get(list: *const RawClapInputEvents, index: u32) -> *const RawClapEventHeader {
    let in_events = match parse_in_clap_events(list) {
        Ok(in_events) => in_events,
        Err(()) => return ptr::null(),
    };

    if let Some(event) = in_events.events.get(index as usize) {
        event.data.as_ptr() as *const RawClapEventHeader
    } else {
        ptr::null()
    }
}

unsafe extern "C" fn try_push(
    list: *const RawClapOutputEvents,
    event: *const RawClapEventHeader,
) -> bool {
    use clap_sys::events::{
        CLAP_EVENT_MIDI, CLAP_EVENT_MIDI2, CLAP_EVENT_MIDI_SYSEX, CLAP_EVENT_NOTE_CHOKE,
        CLAP_EVENT_NOTE_END, CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
        CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_MOD,
        CLAP_EVENT_PARAM_VALUE, CLAP_EVENT_TRANSPORT,
    };

    let out_events = match parse_out_clap_events(list) {
        Ok(out_events) => out_events,
        Err(()) => return false,
    };

    if event.is_null() {
        log::warn!("Received a null clap_event_header_t pointer from plugin");
        return false;
    }

    let mut header = *event;

    let mut data: [u8; std::mem::size_of::<EventTransport>()] =
        { std::mem::MaybeUninit::uninit().assume_init() };

    match header.type_ {
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE | CLAP_EVENT_NOTE_END => {
            header.size = std::mem::size_of::<EventNote>() as u32;

            let event_bytes =
                std::slice::from_raw_parts(event as *const u8, std::mem::size_of::<EventNote>());
            data[0..std::mem::size_of::<EventNote>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_NOTE_EXPRESSION => {
            header.size = std::mem::size_of::<EventNoteExpression>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventNoteExpression>(),
            );
            data[0..std::mem::size_of::<EventNoteExpression>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_PARAM_VALUE => {
            header.size = std::mem::size_of::<EventParamValue>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventParamValue>(),
            );
            data[0..std::mem::size_of::<EventParamValue>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_PARAM_MOD => {
            header.size = std::mem::size_of::<EventParamMod>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventParamMod>(),
            );
            data[0..std::mem::size_of::<EventParamMod>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
            header.size = std::mem::size_of::<EventParamGesture>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventParamGesture>(),
            );
            data[0..std::mem::size_of::<EventParamGesture>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_TRANSPORT => {
            header.size = std::mem::size_of::<EventTransport>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventTransport>(),
            );
            data[0..std::mem::size_of::<EventTransport>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_MIDI => {
            header.size = std::mem::size_of::<EventMidi>() as u32;

            let event_bytes =
                std::slice::from_raw_parts(event as *const u8, std::mem::size_of::<EventMidi>());
            data[0..std::mem::size_of::<EventMidi>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_MIDI_SYSEX => {
            header.size = std::mem::size_of::<EventMidiSysex>() as u32;

            let event_bytes = std::slice::from_raw_parts(
                event as *const u8,
                std::mem::size_of::<EventMidiSysex>(),
            );
            data[0..std::mem::size_of::<EventMidiSysex>()].copy_from_slice(event_bytes);
        }
        CLAP_EVENT_MIDI2 => {
            header.size = std::mem::size_of::<EventMidi2>() as u32;

            let event_bytes =
                std::slice::from_raw_parts(event as *const u8, std::mem::size_of::<EventMidi2>());
            data[0..std::mem::size_of::<EventMidi2>()].copy_from_slice(event_bytes);
        }
        _ => {
            log::warn!("Received an unknown clap_event_header.type from plugin");
            return false;
        }
    }

    if out_events.events.len() >= out_events.events.capacity() {
        log::warn!("Event queue has exceeded its capacity. This will cause an allocation on the audio thread.");
    }

    out_events.events.push(AllocatedEvent { data });

    true
}
