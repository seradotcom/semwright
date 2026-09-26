use reis::{ei, event::Device};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use xkbcommon_dl::{
    XkbCommon, xkb_context, xkb_context_flags, xkb_key_direction, xkb_keymap,
    xkb_keymap_compile_flags, xkb_keymap_format, xkbcommon_option,
};
use xkeysym::{Keysym, key};

const MAX_KEYMAP_BYTES: usize = 1024 * 1024;
const XKB_EVDEV_OFFSET: u32 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModifierState {
    pub depressed: u32,
    pub latched: u32,
    pub locked: u32,
    pub group: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stroke {
    pub keycode: u32,
    pub modifiers: Vec<u32>,
}

pub(crate) struct KeyboardMap {
    xkb: &'static XkbCommon,
    context: *mut xkb_context,
    keymap: *mut xkb_keymap,
}

impl KeyboardMap {
    pub(crate) fn from_device(device: &Device) -> Result<Option<Self>> {
        let Some(source) = device.keymap() else {
            return Ok(None);
        };
        if source.type_ != ei::keyboard::KeymapType::Xkb {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "EIS keyboard keymap is not XKB text v1",
            ));
        }
        let size = usize::try_from(source.size)
            .map_err(|_| Error::invalid("EIS keymap size does not fit usize"))?;
        if size == 0 || size > MAX_KEYMAP_BYTES {
            return Err(Error::invalid(
                "EIS keymap size is outside the bounded limit",
            ));
        }
        let fd = source
            .fd
            .try_clone()
            .map_err(|e| Error::new(ErrorCode::BackendFailed, format!("EIS keymap fd: {e}")))?;
        let mut file = File::from(fd);
        file.seek(SeekFrom::Start(0))
            .map_err(|e| Error::new(ErrorCode::BackendFailed, format!("EIS keymap seek: {e}")))?;
        let mut bytes = vec![0u8; size];
        file.read_exact(&mut bytes)
            .map_err(|e| Error::new(ErrorCode::BackendFailed, format!("EIS keymap read: {e}")))?;
        Self::from_bytes(&bytes).map(Some)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let xkb = xkbcommon_option().ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                "libxkbcommon is unavailable for EIS keycode translation",
            )
        })?;
        // SAFETY: xkbcommon-dl resolves this function and the enum value is valid.
        let context = unsafe { (xkb.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if context.is_null() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Cannot create XKB context",
            ));
        }
        // SAFETY: bytes is valid for the duration of the call; xkbcommon parses the buffer.
        let keymap = unsafe {
            (xkb.xkb_keymap_new_from_buffer)(
                context,
                bytes.as_ptr().cast(),
                bytes.len(),
                xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        if keymap.is_null() {
            // SAFETY: context was returned by xkb_context_new and is still owned here.
            unsafe { (xkb.xkb_context_unref)(context) };
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Cannot parse EIS XKB keymap",
            ));
        }
        Ok(Self {
            xkb,
            context,
            keymap,
        })
    }
    pub(crate) fn stroke_for_keysym(&self, keysym: u32, current: ModifierState) -> Result<Stroke> {
        let (shift, level3) = self.text_modifier_keys();
        let mut combinations: Vec<Vec<u32>> = vec![vec![]];
        if let Some(code) = shift {
            combinations.push(vec![code]);
        }
        if let Some(code) = level3 {
            combinations.push(vec![code]);
            if let Some(shift) = shift {
                combinations.push(vec![shift, code]);
            }
        }
        for modifiers in combinations {
            if let Some(keycode) = self.find_keysym(keysym, current, &modifiers)? {
                return Ok(Stroke {
                    keycode: to_evdev(keycode)?,
                    modifiers: modifiers
                        .into_iter()
                        .map(to_evdev)
                        .collect::<Result<Vec<_>>>()?,
                });
            }
        }
        Err(Error::new(
            ErrorCode::Unsupported,
            format!("Keysym 0x{keysym:x} is not directly representable by the EIS XKB keymap"),
        ))
    }

    pub(crate) fn stroke_for_char(&self, value: char, current: ModifierState) -> Result<Stroke> {
        let keysym = Keysym::from_char(value);
        if keysym == Keysym::NoSymbol {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Unicode scalar has no XKB keysym",
            ));
        }
        self.stroke_for_keysym(keysym.raw(), current)
    }

    fn text_modifier_keys(&self) -> (Option<u32>, Option<u32>) {
        let mut shift = None;
        let mut level3 = None;
        for code in self.min_keycode()..=self.max_keycode() {
            for symbol in self.base_symbols(code) {
                if shift.is_none() && matches!(symbol, key::Shift_L | key::Shift_R) {
                    shift = Some(code);
                }
                if level3.is_none()
                    && matches!(
                        symbol,
                        key::ISO_Level3_Shift | key::ISO_Level5_Shift | key::Mode_switch
                    )
                {
                    level3 = Some(code);
                }
            }
            if shift.is_some() && level3.is_some() {
                break;
            }
        }
        (shift, level3)
    }
    fn base_symbols(&self, code: u32) -> Vec<u32> {
        // SAFETY: self.keymap is a live xkb_keymap owned by this object.
        let layouts = unsafe { (self.xkb.xkb_keymap_num_layouts_for_key)(self.keymap, code) };
        let mut result = Vec::new();
        for layout in 0..layouts {
            let mut symbols = std::ptr::null();
            // SAFETY: xkbcommon writes a borrowed pointer valid while the keymap is alive.
            let count = unsafe {
                (self.xkb.xkb_keymap_key_get_syms_by_level)(
                    self.keymap,
                    code,
                    layout,
                    0,
                    &mut symbols,
                )
            };
            if count > 0 && !symbols.is_null() {
                // SAFETY: xkbcommon returned count entries owned by the keymap.
                let slice = unsafe { std::slice::from_raw_parts(symbols, count as usize) };
                result.extend_from_slice(slice);
            }
        }
        result
    }

    fn find_keysym(
        &self,
        target: u32,
        current: ModifierState,
        modifiers: &[u32],
    ) -> Result<Option<u32>> {
        // SAFETY: self.keymap is live for the lifetime of the returned state.
        let state = unsafe { (self.xkb.xkb_state_new)(self.keymap) };
        if state.is_null() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Cannot create XKB state",
            ));
        }
        let found = (|| {
            // SAFETY: state is live and masks/group are protocol-provided XKB state values.
            unsafe {
                (self.xkb.xkb_state_update_mask)(
                    state,
                    current.depressed,
                    current.latched,
                    current.locked,
                    0,
                    0,
                    current.group,
                );
                for modifier in modifiers {
                    (self.xkb.xkb_state_update_key)(
                        state,
                        *modifier,
                        xkb_key_direction::XKB_KEY_DOWN,
                    );
                }
            }
            for code in self.min_keycode()..=self.max_keycode() {
                // SAFETY: state is live and keycode is inside the keymap advertised range.
                let symbol = unsafe { (self.xkb.xkb_state_key_get_one_sym)(state, code) };
                if symbol == target {
                    return Some(code);
                }
            }
            None
        })();
        // SAFETY: state was created above and has not been released yet.
        unsafe { (self.xkb.xkb_state_unref)(state) };
        Ok(found)
    }

    fn min_keycode(&self) -> u32 {
        // SAFETY: self.keymap is live.
        unsafe { (self.xkb.xkb_keymap_min_keycode)(self.keymap) }
    }

    fn max_keycode(&self) -> u32 {
        // SAFETY: self.keymap is live.
        unsafe { (self.xkb.xkb_keymap_max_keycode)(self.keymap) }
    }
}
impl Drop for KeyboardMap {
    fn drop(&mut self) {
        // SAFETY: both pointers are owned by this object and released exactly once.
        unsafe {
            (self.xkb.xkb_keymap_unref)(self.keymap);
            (self.xkb.xkb_context_unref)(self.context);
        }
    }
}

fn to_evdev(xkb_keycode: u32) -> Result<u32> {
    xkb_keycode.checked_sub(XKB_EVDEV_OFFSET).ok_or_else(|| {
        Error::new(
            ErrorCode::BackendFailed,
            "XKB keycode is below the evdev offset",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_offset_is_checked() {
        assert_eq!(to_evdev(38).unwrap(), 30);
        assert!(to_evdev(7).is_err());
    }

    #[test]
    fn modifier_state_defaults_to_neutral() {
        assert_eq!(ModifierState::default().group, 0);
        assert_eq!(ModifierState::default().depressed, 0);
    }
}
