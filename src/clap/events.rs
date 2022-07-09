use std::ffi::c_void;
use std::pin::Pin;
use std::ptr;

use clap_sys::events::clap_event_header as RawClapEventHeader;
use clap_sys::events::clap_event_midi as RawClapEventMidi;
use clap_sys::events::clap_event_midi2 as RawClapEventMidi2;
use clap_sys::events::clap_event_note as RawClapEventNote;
use clap_sys::events::clap_event_note_expression as RawClapEventNoteExpression;
use clap_sys::events::clap_event_param_gesture as RawClapEventParamGesture;
use clap_sys::events::clap_event_param_mod as RawClapEventParamMod;
use clap_sys::events::clap_event_param_value as RawClapEventParamValue;
use clap_sys::events::clap_event_transport as RawClapEventTransport;
use clap_sys::events::clap_input_events as RawClapInputEvents;
use clap_sys::events::clap_output_events as RawClapOutputEvents;

use crate::plugin::events::event_queue::ProcEvent;
use crate::plugin::events::{
    EventMidi, EventMidi2, EventNote, EventNoteExpression, EventParamGesture, EventParamMod,
    EventParamValue, EventTransport,
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
        let event_ref = event.raw_pointer();
        event_ref as *const RawClapEventHeader
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

    let header = *event;

    let event: ProcEvent = match header.type_ {
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE | CLAP_EVENT_NOTE_END => {
            EventNote::from_raw(*(event as *const RawClapEventNote)).into()
        }
        CLAP_EVENT_NOTE_EXPRESSION => {
            EventNoteExpression::from_raw(*(event as *const RawClapEventNoteExpression)).into()
        }
        CLAP_EVENT_PARAM_VALUE => ProcEvent::param_value(
            EventParamValue::from_raw(*(event as *const RawClapEventParamValue)),
            None,
        ),
        CLAP_EVENT_PARAM_MOD => ProcEvent::param_mod(
            EventParamMod::from_raw(*(event as *const RawClapEventParamMod)),
            None,
        ),
        CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
            EventParamGesture::from_raw(*(event as *const RawClapEventParamGesture)).into()
        }
        CLAP_EVENT_TRANSPORT => {
            EventTransport::from_raw(*(event as *const RawClapEventTransport)).into()
        }
        CLAP_EVENT_MIDI => EventMidi::from_raw(*(event as *const RawClapEventMidi)).into(),
        CLAP_EVENT_MIDI_SYSEX => {
            log::warn!("Received an unsupported CLAP_EVENT_MIDI_SYSEX event from plugin");
            return false;
        }
        CLAP_EVENT_MIDI2 => EventMidi2::from_raw(*(event as *const RawClapEventMidi2)).into(),
        _ => {
            log::warn!("Received an unknown clap_event_header.type from plugin");
            return false;
        }
    };

    if out_events.events.len() >= out_events.events.capacity() {
        log::warn!("Event queue has exceeded its capacity. This will cause an allocation on the audio thread.");
    }

    out_events.events.push(event);

    true
}
