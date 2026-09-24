use semwright_types::{Error, ErrorCode, Result};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VIRTUAL_KEY,
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

pub fn mouse_move_relative(dx: i32, dy: i32) -> Result<()> {
    if !(-10_000..=10_000).contains(&dx) || !(-10_000..=10_000).contains(&dy) {
        return Err(Error::invalid(
            "Relative mouse delta exceeds contract bounds",
        ));
    }
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[input])
}

pub fn click(button: &str) -> Result<()> {
    let (down, up) = match button {
        "left" => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        _ => return Err(Error::invalid("Unsupported mouse button")),
    };
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
    send(&[make(down), make(up)])
}

pub fn scroll(dx: i32, dy: i32) -> Result<()> {
    if !(-1_000..=1_000).contains(&dx) || !(-1_000..=1_000).contains(&dy) {
        return Err(Error::invalid("Scroll delta exceeds contract bounds"));
    }
    let mut inputs = Vec::with_capacity(2);
    let make = |delta: i32, flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    if dx != 0 {
        inputs.push(make(dx, MOUSEEVENTF_HWHEEL));
    }
    if dy != 0 {
        inputs.push(make(dy, MOUSEEVENTF_WHEEL));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    send(&inputs)
}
