use dropseed_plugin_api::ParamID;
use fnv::FnvHashMap;
use std::collections::hash_map::Entry::{Occupied, Vacant};

use super::{AutomationIoEvent, AutomationIoEventType, PluginIoEvent};

/// Sanitizes a plugin's event output stream, by wrapping an event iterator.
///
/// For now, this only means de-duplicating BeginGesture / EndGesture events.
pub struct PluginEventOutputSanitizer {
    is_adjusting_parameter: FnvHashMap<ParamID, bool>,
}

impl PluginEventOutputSanitizer {
    pub fn new(param_capacity: usize) -> Self {
        let mut is_adjusting_parameter = FnvHashMap::default();
        is_adjusting_parameter.reserve(param_capacity * 2);

        Self { is_adjusting_parameter }
    }

    #[allow(unused)]
    pub fn reset(&mut self) {
        self.is_adjusting_parameter.clear()
    }

    #[inline]
    pub fn sanitize<I>(&mut self, iterator: I) -> ParamOutputSanitizerIter<I>
    where
        I: Iterator<Item = PluginIoEvent>,
    {
        ParamOutputSanitizerIter { sanitizer: self, iterator }
    }
}

pub struct ParamOutputSanitizerIter<'a, I> {
    sanitizer: &'a mut PluginEventOutputSanitizer,
    iterator: I,
}

impl<'a, I> Iterator for ParamOutputSanitizerIter<'a, I>
where
    I: Iterator<Item = PluginIoEvent>,
{
    type Item = PluginIoEvent;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for event in self.iterator.by_ref() {
            if let PluginIoEvent::AutomationEvent {
                cookie: _,
                event: AutomationIoEvent { parameter_id, event_type, .. },
            } = &event
            {
                let is_beginning = match event_type {
                    AutomationIoEventType::BeginGesture => true,
                    AutomationIoEventType::EndGesture => false,
                    _ => return Some(event),
                };

                match self.sanitizer.is_adjusting_parameter.entry(ParamID(*parameter_id)) {
                    Occupied(mut o) => {
                        if *o.get() == is_beginning {
                            continue;
                        }
                        o.insert(is_beginning);
                    }
                    Vacant(v) => {
                        v.insert(is_beginning);
                    }
                };
            }

            return Some(event);
        }

        None
    }
}
