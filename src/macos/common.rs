#![allow(clippy::upper_case_acronyms)]
use crate::macos::keyboard::Keyboard;
use crate::rdev::{Button, Event, EventType};
use core::ptr::NonNull;
use lazy_static::lazy_static;
use objc2_core_graphics::{CGEvent, CGEventField, CGEventFlags, CGEventType};
use std::convert::TryInto;
use std::sync::Mutex;
use std::time::SystemTime;

use crate::macos::keycodes::key_from_code_or_name;

use super::keycodes::key_from_special_key;

lazy_static! {
    pub static ref LAST_FLAGS: Mutex<CGEventFlags> = Mutex::new(CGEventFlags(0));
    pub static ref KEYBOARD_STATE: Mutex<Keyboard> = Mutex::new(Keyboard::new().unwrap());
}

pub fn set_is_main_thread(b: bool) {
    KEYBOARD_STATE.lock().unwrap().set_is_main_thread(b);
}

/// The device-dependent flag bit (`NX_DEVICE*KEYMASK`) of a modifier key,
/// by virtual keycode. `None` for keys without one, e.g. Caps Lock, which
/// only toggles a lock state.
fn device_mask(code: u16) -> Option<u64> {
    Some(match code {
        59 => 0x0000_0001, // ControlLeft
        56 => 0x0000_0002, // ShiftLeft
        60 => 0x0000_0004, // ShiftRight
        55 => 0x0000_0008, // MetaLeft
        54 => 0x0000_0010, // MetaRight
        58 => 0x0000_0020, // Alt
        61 => 0x0000_0040, // AltGr
        62 => 0x0000_2000, // ControlRight
        _ => return None,
    })
}

/// The modifier flags after the modifier key `code` went down or up: its
/// device bit set or cleared, and the shared flag of its group (Shift,
/// Control, …) set while either key of the group is still held. What a
/// physical key produces, so a listener tells simulated presses and
/// releases apart the same way. Other keys leave the flags as they are.
pub(crate) fn modifier_flags(flags: CGEventFlags, code: u16, down: bool) -> CGEventFlags {
    const GROUPS: [(u64, CGEventFlags); 4] = [
        (0x0000_0002 | 0x0000_0004, CGEventFlags::MaskShift),
        (0x0000_0001 | 0x0000_2000, CGEventFlags::MaskControl),
        (0x0000_0020 | 0x0000_0040, CGEventFlags::MaskAlternate),
        (0x0000_0008 | 0x0000_0010, CGEventFlags::MaskCommand),
    ];
    let Some(mask) = device_mask(code) else {
        return flags;
    };
    let mut bits = if down {
        flags.0 | mask
    } else {
        flags.0 & !mask
    };
    for (devices, shared) in GROUPS {
        if devices & mask != 0 {
            if bits & devices != 0 {
                bits |= shared.0;
            } else {
                bits &= !shared.0;
            }
        }
    }
    CGEventFlags(bits)
}

/// Middle and side buttons: macOS numbers them from 2 (middle) up, after
/// left (0) and right (1). Anything past the middle button is passed on
/// with that number.
unsafe fn other_button(cg_event: NonNull<CGEvent>) -> Button {
    let number = unsafe {
        CGEvent::integer_value_field(
            Some(cg_event.as_ref()),
            CGEventField::MouseEventButtonNumber,
        )
    };
    match number {
        2 => Button::Middle,
        n => Button::Unknown(n.try_into().unwrap_or(u8::MAX)),
    }
}

pub unsafe fn convert(
    _type: CGEventType,
    cg_event: NonNull<CGEvent>,
    keyboard_state: &mut Keyboard,
) -> Option<Event> {
    unsafe {
        let option_type = match _type {
            CGEventType::LeftMouseDown => Some(EventType::ButtonPress(Button::Left)),
            CGEventType::LeftMouseUp => Some(EventType::ButtonRelease(Button::Left)),
            CGEventType::RightMouseDown => Some(EventType::ButtonPress(Button::Right)),
            CGEventType::RightMouseUp => Some(EventType::ButtonRelease(Button::Right)),
            CGEventType::OtherMouseDown => Some(EventType::ButtonPress(other_button(cg_event))),
            CGEventType::OtherMouseUp => Some(EventType::ButtonRelease(other_button(cg_event))),
            CGEventType::MouseMoved => {
                let point = CGEvent::location(Some(cg_event.as_ref()));
                // let point = cg_event.location();
                Some(EventType::MouseMove {
                    x: point.x,
                    y: point.y,
                })
            }
            CGEventType::LeftMouseDragged => {
                let point = CGEvent::location(Some(cg_event.as_ref()));
                Some(EventType::MouseMove {
                    x: point.x,
                    y: point.y,
                })
            }
            CGEventType::RightMouseDragged => {
                let point = CGEvent::location(Some(cg_event.as_ref()));
                Some(EventType::MouseMove {
                    x: point.x,
                    y: point.y,
                })
            }
            CGEventType::OtherMouseDragged => {
                let point = CGEvent::location(Some(cg_event.as_ref()));
                Some(EventType::MouseMove {
                    x: point.x,
                    y: point.y,
                })
            }
            CGEventType::KeyDown => {
                // println!("Received {cg_event:?}");
                let code = CGEvent::integer_value_field(
                    Some(cg_event.as_ref()),
                    CGEventField::KeyboardEventKeycode,
                );
                let flags = CGEvent::flags(Some(cg_event.as_ref()));
                let name = keyboard_state.create_string_for_key(code, flags);
                let key = key_from_code_or_name(code.try_into().ok()?, name.as_deref());
                Some(EventType::KeyPress(key))
            }
            CGEventType::KeyUp => {
                let code = CGEvent::integer_value_field(
                    Some(cg_event.as_ref()),
                    CGEventField::KeyboardEventKeycode,
                );
                let flags = CGEvent::flags(Some(cg_event.as_ref()));
                let name = keyboard_state.create_string_for_key(code, flags);
                let key = key_from_code_or_name(code.try_into().ok()?, name.as_deref());
                Some(EventType::KeyRelease(key))
            }
            CGEventType::FlagsChanged => {
                let code = CGEvent::integer_value_field(
                    Some(cg_event.as_ref()),
                    CGEventField::KeyboardEventKeycode,
                );
                let code = code.try_into().ok()?;
                let flags = CGEvent::flags(Some(cg_event.as_ref()));
                let key = key_from_code_or_name(code, None);
                // Each modifier key has its own device bit in the flags, so the
                // event itself says whether this key went down or up. The shared
                // Shift/Control/… flags can't: with both Shift keys held,
                // releasing one leaves the flag set. `LAST_FLAGS` only keeps
                // the physical state for `simulate`, which stamps it onto the
                // key events it posts; the detection never reads it.
                *LAST_FLAGS.lock().unwrap() = flags;
                let held = flags.0 & device_mask(code)? != 0;
                Some(if held {
                    EventType::KeyPress(key)
                } else {
                    EventType::KeyRelease(key)
                })
            }
            CGEventType::ScrollWheel => {
                let delta_y = CGEvent::integer_value_field(
                    Some(cg_event.as_ref()),
                    CGEventField::ScrollWheelEventDeltaAxis1,
                );
                let delta_x = CGEvent::integer_value_field(
                    Some(cg_event.as_ref()),
                    CGEventField::ScrollWheelEventDeltaAxis2,
                );
                Some(EventType::Wheel { delta_x, delta_y })
            }
            CGEventType(14) => {
                // Core graphics doesnt support NX_SYSDEFINED yet

                let subtype =
                    CGEvent::integer_value_field(Some(cg_event.as_ref()), CGEventField(99));
                let data1 =
                    CGEvent::integer_value_field(Some(cg_event.as_ref()), CGEventField(149));
                let key_flags = data1 & 0x0000ffff;
                let key_pressed = ((key_flags & 0xff00) >> 8) == 0xa;
                let _key_repeat = (key_flags & 0x1) == 0x1;
                let key_code = (data1 & 0xffff0000) >> 16;

                // Mouse buttons like middle click/back/forward are subtype 7
                // Subtype 8 means keyboard event
                if subtype != 8 {
                    return None;
                }

                // println!("Received {key_code:?}");
                if let Some(code) = key_from_special_key(key_code.try_into().ok()?) {
                    if key_pressed {
                        Some(EventType::KeyPress(code))
                    } else {
                        Some(EventType::KeyRelease(code))
                    }
                } else {
                    // If we don't handle the key avoid creating an event since it can create duplicates with other keys
                    None
                }
            }
            _ev => {
                // println!("Received {_ev:?}");
                None
            }
        };
        if let Some(event_type) = option_type {
            let name = match event_type {
                EventType::KeyPress(_) => {
                    let code = CGEvent::integer_value_field(
                        Some(cg_event.as_ref()),
                        CGEventField::KeyboardEventKeycode,
                    );
                    let flags = CGEvent::flags(Some(cg_event.as_ref()));
                    keyboard_state.create_string_for_key(code, flags)
                }
                _ => None,
            };
            return Some(Event {
                event_type,
                time: SystemTime::now(),
                name,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: u64 = 1 << 17;
    const LEFT_SHIFT: u64 = 0x2;
    const RIGHT_SHIFT: u64 = 0x4;

    fn flags_changed(code: u16, flags: u64) -> Option<EventType> {
        let event = CGEvent::new_keyboard_event(None, code, true).unwrap();
        CGEvent::set_type(Some(&event), CGEventType::FlagsChanged);
        CGEvent::set_flags(Some(&event), CGEventFlags(flags));
        let mut keyboard = Keyboard::new().unwrap();
        unsafe {
            convert(
                CGEventType::FlagsChanged,
                NonNull::from(&*event),
                &mut keyboard,
            )
        }
        .map(|event| event.event_type)
    }

    #[test]
    #[serial_test::serial]
    fn tells_both_shift_keys_apart() {
        use crate::rdev::Key::{ShiftLeft, ShiftRight};
        assert_eq!(
            flags_changed(56, SHIFT | LEFT_SHIFT),
            Some(EventType::KeyPress(ShiftLeft))
        );
        assert_eq!(
            flags_changed(60, SHIFT | LEFT_SHIFT | RIGHT_SHIFT),
            Some(EventType::KeyPress(ShiftRight))
        );
        assert_eq!(
            flags_changed(56, SHIFT | RIGHT_SHIFT),
            Some(EventType::KeyRelease(ShiftLeft))
        );
        assert_eq!(
            flags_changed(60, 0),
            Some(EventType::KeyRelease(ShiftRight))
        );
    }

    #[test]
    fn keeps_the_shared_flag_while_either_key_is_held() {
        let shift = CGEventFlags::MaskShift.0;
        let left = modifier_flags(CGEventFlags(0), 56, true);
        assert_eq!(left.0, shift | LEFT_SHIFT);
        let both = modifier_flags(left, 60, true);
        assert_eq!(both.0, shift | LEFT_SHIFT | RIGHT_SHIFT);
        let right = modifier_flags(both, 56, false);
        assert_eq!(right.0, shift | RIGHT_SHIFT);
        assert_eq!(modifier_flags(right, 60, false).0, 0);
        // Keys without a device bit leave the flags alone.
        assert_eq!(modifier_flags(right, 57, true).0, right.0);
    }
}
