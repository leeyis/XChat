//! Bounded input packets. The caller must hold the session lock through authorization and execution.
use super::model::Result;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Move {
        x: f64,
        y: f64,
    },
    Button {
        x: f64,
        y: f64,
        button: u8,
        down: bool,
    },
    Wheel {
        x: f64,
        y: f64,
        delta: i32,
        horizontal: bool,
    },
    Key {
        code: String,
        down: bool,
    },
    Release,
    KeepAlive,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub grant: String,
    pub sequence: u64,
    pub event: Event,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub scan: u16,
    pub extended: bool,
}

/// USB-style browser codes map to Windows set-1 scan codes; arbitrary virtual keys are not accepted.
pub fn key(code: &str) -> Option<Key> {
    let (scan, extended) = match code {
        "Escape" => (0x01, false),
        "Backspace" => (0x0e, false),
        "Tab" => (0x0f, false),
        "Enter" => (0x1c, false),
        "Space" => (0x39, false),
        "CapsLock" => (0x3a, false),
        "ShiftLeft" => (0x2a, false),
        "ShiftRight" => (0x36, false),
        "ControlLeft" => (0x1d, false),
        "ControlRight" => (0x1d, true),
        "AltLeft" => (0x38, false),
        "AltRight" => (0x38, true),
        "MetaLeft" => (0x5b, true),
        "MetaRight" => (0x5c, true),
        "ContextMenu" => (0x5d, true),
        "ArrowLeft" => (0x4b, true),
        "ArrowRight" => (0x4d, true),
        "ArrowUp" => (0x48, true),
        "ArrowDown" => (0x50, true),
        "Home" => (0x47, true),
        "End" => (0x4f, true),
        "PageUp" => (0x49, true),
        "PageDown" => (0x51, true),
        "Insert" => (0x52, true),
        "Delete" => (0x53, true),
        "Minus" => (0x0c, false),
        "Equal" => (0x0d, false),
        "BracketLeft" => (0x1a, false),
        "BracketRight" => (0x1b, false),
        "Semicolon" => (0x27, false),
        "Quote" => (0x28, false),
        "Backquote" => (0x29, false),
        "Backslash" => (0x2b, false),
        "Comma" => (0x33, false),
        "Period" => (0x34, false),
        "Slash" => (0x35, false),
        "NumpadEnter" => (0x1c, true),
        "NumpadDivide" => (0x35, true),
        "NumpadMultiply" => (0x37, false),
        "NumpadSubtract" => (0x4a, false),
        "NumpadAdd" => (0x4e, false),
        "NumpadDecimal" => (0x53, false),
        "NumLock" => (0x45, false),
        _ => {
            if let Some(letter) = code.strip_prefix("Key").filter(|s| s.len() == 1) {
                let scans = [
                    0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32,
                    0x31, 0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
                ];
                let value = letter.as_bytes()[0];
                if !value.is_ascii_uppercase() {
                    return None;
                }
                (scans[(value - b'A') as usize], false)
            } else if let Some(value) = code
                .strip_prefix("Digit")
                .filter(|s| s.len() == 1)
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n < 10)
            {
                (if value == 0 { 0x0b } else { value as u16 + 1 }, false)
            } else if let Some(value) = code
                .strip_prefix("Numpad")
                .filter(|s| s.len() == 1)
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n < 10)
            {
                (
                    [0x52, 0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, 0x48, 0x49][value],
                    false,
                )
            } else if let Some(value) = code
                .strip_prefix('F')
                .and_then(|s| s.parse::<u16>().ok())
                .filter(|n| (1..=12).contains(n))
            {
                (
                    match value {
                        11 => 0x57,
                        12 => 0x58,
                        n => 0x3a + n,
                    },
                    false,
                )
            } else {
                return None;
            }
        }
    };
    Some(Key { scan, extended })
}

impl Event {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Move { x, y } | Self::Button { x, y, .. } | Self::Wheel { x, y, .. } => {
                if !x.is_finite()
                    || !y.is_finite()
                    || !(0.0..=1.0).contains(x)
                    || !(0.0..=1.0).contains(y)
                {
                    return Err("鼠标坐标超出共享屏幕".into());
                }
            }
            Self::Key { code, .. } if key(code).is_none() => return Err("不支持的键盘按键".into()),
            _ => (),
        }
        if matches!(self, Self::Button {button,..} if *button>2)
            || matches!(self, Self::Wheel {delta,..} if delta.unsigned_abs()>1200)
        {
            return Err("输入参数超出允许范围".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Held {
    pub keys: HashSet<Key>,
    pub buttons: HashSet<u8>,
    pub sequence: u64,
}
impl Held {
    pub fn reset_sequence(&mut self) {
        self.sequence = 0;
    }
    pub fn validate(&self, packet: &Packet, grant: &str) -> Result<()> {
        if packet.grant != grant
            || packet.sequence <= self.sequence
            || packet.sequence - self.sequence > 10000
        {
            return Err("远程输入授权已失效或序号过期".into());
        }
        packet.event.validate()?;
        if matches!(&packet.event,Event::Key{code,down:true} if self.keys.len()>=32 && !self.keys.contains(&key(code).unwrap()))
        {
            return Err("同时按下的按键过多".into());
        }
        Ok(())
    }
    pub fn record(&mut self, packet: &Packet) {
        self.sequence = packet.sequence;
        match &packet.event {
            Event::Key { code, down } => {
                let key = key(code).unwrap();
                if *down {
                    self.keys.insert(key);
                } else {
                    self.keys.remove(&key);
                }
            }
            Event::Button { button, down, .. } => {
                if *down {
                    self.buttons.insert(*button);
                } else {
                    self.buttons.remove(button);
                }
            }
            Event::Release => {
                self.keys.clear();
                self.buttons.clear();
            }
            _ => (),
        }
    }
}

/// Convert normalized coordinates in one monitor into absolute virtual-desktop coordinates.
#[cfg(any(test, all(feature = "desktop", target_os = "windows")))]
pub fn coordinates(
    x: f64,
    y: f64,
    screen: (i32, i32, u32, u32),
    desktop: (i32, i32, i32, i32),
) -> Result<(i32, i32)> {
    Event::Move { x, y }.validate()?;
    let (sx, sy, sw, sh) = screen;
    let (dx, dy, dw, dh) = desktop;
    if sw == 0 || sh == 0 || dw <= 1 || dh <= 1 {
        return Err("显示器尺寸无效".into());
    }
    let px = sx as f64 + x * (sw - 1) as f64;
    let py = sy as f64 + y * (sh - 1) as f64;
    if px < (dx as f64)
        || py < (dy as f64)
        || px >= (dx as f64 + dw as f64)
        || py >= (dy as f64 + dh as f64)
    {
        return Err("共享屏幕已发生变化，请重新选择".into());
    }
    Ok((
        ((px - dx as f64) / (dw - 1) as f64 * 65535.0).round() as i32,
        ((py - dy as f64) / (dh - 1) as f64 * 65535.0).round() as i32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inputs_reject_old_grants_replays_nonfinite_and_out_of_bounds() {
        let mut held = Held::default();
        let mut p = Packet {
            grant: "current".into(),
            sequence: 1,
            event: Event::Key {
                code: "ShiftLeft".into(),
                down: true,
            },
        };
        held.validate(&p, "current").unwrap();
        held.record(&p);
        assert_eq!(held.keys.len(), 1);
        assert!(held.validate(&p, "current").is_err());
        p.sequence = 2;
        assert!(held.validate(&p, "revoked").is_err());
        p.event = Event::Move {
            x: f64::NAN,
            y: 0.0,
        };
        assert!(held.validate(&p, "current").is_err());
        p.event = Event::Move { x: 1.1, y: 0.0 };
        assert!(held.validate(&p, "current").is_err());
        p.event = Event::Key {
            code: "ExecuteArbitrary".into(),
            down: true,
        };
        assert!(held.validate(&p, "current").is_err());
        p.event = Event::Release;
        held.record(&p);
        assert!(held.keys.is_empty());
    }
    #[test]
    fn coordinates_cover_negative_origin_and_selected_monitor_only() {
        assert_eq!(
            coordinates(0.0, 0.0, (-1920, 0, 1920, 1080), (-1920, 0, 3840, 1080)).unwrap(),
            (0, 0)
        );
        let right = coordinates(1.0, 1.0, (-1920, 0, 1920, 1080), (-1920, 0, 3840, 1080)).unwrap();
        assert!(right.0 < 32768);
        assert_eq!(right.1, 65535);
        assert!(coordinates(1.0, 1.0, (0, 0, 5000, 1080), (-1920, 0, 3840, 1080)).is_err());
    }
}
