use semwright_types::{Error, ErrorCode, Result};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};

fn send(inputs: &[INPUT]) -> Result<()> {
    // SAFETY: all INPUT values are initialized, have the correct discriminant and live for call.
    let written = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    if written != inputs.len() {
        // SendInput deliberately does not expose a distinct UIPI error. Treat refusal as a
        // permission/unavailability boundary and never attempt an elevation bypass.
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows refused synthetic input (UIPI/focus/integrity boundary or input failure)",
        ));
    }
    Ok(())
}

fn unicode(unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: KEYEVENTF_UNICODE
                    | if key_up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn type_unicode(text: &str) -> Result<()> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > 16_384 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Synthetic text exceeds UTF-16 budget",
        ));
    }
    let mut batch = Vec::with_capacity(units.len() * 2);
    for unit in units {
        batch.push(unicode(unit, false));
        batch.push(unicode(unit, true));
    }
    send(&batch)
}

pub fn mouse_move_absolute(normalized_x: i32, normalized_y: i32) -> Result<()> {
    if !(0..=65_535).contains(&normalized_x) || !(0..=65_535).contains(&normalized_y) {
        return Err(Error::invalid(
            "Absolute mouse coordinates must be normalized to 0..65535",
        ));
    }
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: normalized_x,
                dy: normalized_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[input])
}

pub fn left_click() -> Result<()> {
    let make = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[make(MOUSEEVENTF_LEFTDOWN), make(MOUSEEVENTF_LEFTUP)])
}

pub fn wheel(delta: i32) -> Result<()> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[input])
}
