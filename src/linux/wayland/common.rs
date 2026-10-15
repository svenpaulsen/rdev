/// Linux side buttons (`BTN_SIDE` 275 … `BTN_TASK` 279) carry the X11
/// button numbers 8 … 12 in `Button::Unknown`, so both Linux backends
/// report a mouse's back (8) and forward (9) buttons alike.
const SIDE_BUTTONS: std::ops::RangeInclusive<u32> = 275..=279;
const X11_OFFSET: u32 = 267;

/// The `Button::Unknown` number of a side button's evdev code.
pub fn side_button_number(code: u32) -> Option<u8> {
    SIDE_BUTTONS
        .contains(&code)
        .then(|| (code - X11_OFFSET) as u8)
}

/// The evdev code of a side button's `Button::Unknown` number.
pub fn side_button_code(number: u8) -> Option<u16> {
    let code = u32::from(number) + X11_OFFSET;
    SIDE_BUTTONS.contains(&code).then_some(code as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_side_buttons_like_x11() {
        assert_eq!(side_button_number(275), Some(8));
        assert_eq!(side_button_number(276), Some(9));
        assert_eq!(side_button_number(274), None);
        assert_eq!(side_button_code(9), Some(276));
        assert_eq!(side_button_code(3), None);
    }
}
