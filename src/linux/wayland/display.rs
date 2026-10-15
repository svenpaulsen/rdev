use crate::rdev::DisplayError;

/// Wayland has no protocol-independent way for a client to read the
/// screen size; report that instead of panicking.
pub fn display_size() -> Result<(u64, u64), DisplayError> {
    Err(DisplayError::NoDisplay)
}
