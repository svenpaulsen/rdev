use crate::rdev::{Event, ListenError};
use crate::windows::common::{
    HOOK, HookError, convert, message_loop, set_key_hook, set_mouse_hook,
};
use std::os::raw::c_int;
use std::time::SystemTime;
use winapi::shared::minwindef::{LPARAM, LRESULT, WPARAM};
use winapi::um::winuser::{CallNextHookEx, HC_ACTION};

static mut GLOBAL_CALLBACK: Option<Box<dyn FnMut(Event)>> = None;

impl From<HookError> for ListenError {
    fn from(error: HookError) -> Self {
        match error {
            HookError::Mouse(code) => ListenError::MouseHookError(code),
            HookError::Key(code) => ListenError::KeyHookError(code),
        }
    }
}

unsafe extern "system" fn raw_callback(code: c_int, param: WPARAM, lpdata: LPARAM) -> LRESULT {
    unsafe {
        if code == HC_ACTION {
            let opt = convert(param, lpdata);
            if let Some(event_type) = opt {
                // No name resolution here: `get_name` runs `ToUnicodeEx`, which
                // mutates the layout's global dead-key state and breaks dead
                // keys (`, ´, ^, …) in whatever app has focus.
                let event = Event {
                    event_type,
                    time: SystemTime::now(),
                    name: None,
                };
                let ptr = &raw mut GLOBAL_CALLBACK;
                if let Some(callback) = &mut *ptr {
                    callback(event);
                }
            }
        }
        CallNextHookEx(HOOK, code, param, lpdata)
    }
}

pub fn listen<T>(callback: T) -> Result<(), ListenError>
where
    T: FnMut(Event) + 'static,
{
    unsafe {
        GLOBAL_CALLBACK = Some(Box::new(callback));
        set_key_hook(raw_callback)?;
        set_mouse_hook(raw_callback)?;

        message_loop().map_err(ListenError::MessageLoopError)
    }
}
