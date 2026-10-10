//! Remote assistance has its own capture lifecycle, independent of screenshot/editor modules.
use super::{
    input,
    model::{Quality, Result, Screen},
};

pub fn native_supported() -> bool {
    cfg!(all(feature = "desktop", target_os = "windows"))
}

#[cfg(all(feature = "desktop", target_os = "windows"))]
mod windows {
    use super::*;
    use windows_sys::Win32::{
        System::StationsAndDesktops::*,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    pub fn interactive() -> bool {
        // Only inspect the active desktop. Never switch or unlock the user's desktop.
        unsafe {
            let handle = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
            if handle.is_null() {
                return false;
            }
            let mut name = [0u16; 256];
            let mut needed = 0;
            let read = GetUserObjectInformationW(
                handle,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                (name.len() * 2) as u32,
                &mut needed,
            );
            CloseDesktop(handle);
            read != 0
                && String::from_utf16_lossy(
                    &name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())],
                )
                .eq_ignore_ascii_case("default")
        }
    }
    fn monitor(id: &str) -> Result<xcap::Monitor> {
        xcap::Monitor::all()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.id().ok().is_some_and(|n| n.to_string() == id))
            .ok_or_else(|| "所选显示器已断开，请重新选择共享屏幕".into())
    }
    pub fn screens() -> Result<Vec<Screen>> {
        if !interactive() {
            return Err("当前桌面不可共享，请解锁后重新发起".into());
        }
        xcap::Monitor::all()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|m| {
                Ok(Screen {
                    id: m.id().map_err(|e| e.to_string())?.to_string(),
                    name: m.name().map_err(|e| e.to_string())?,
                    width: m.width().map_err(|e| e.to_string())?,
                    height: m.height().map_err(|e| e.to_string())?,
                })
            })
            .collect()
    }
    pub fn frame(screen: &Screen, quality: &Quality) -> Result<Vec<u8>> {
        if !interactive() {
            return Err("共享的电脑已锁屏".into());
        }
        let monitor = monitor(&screen.id)?;
        if monitor.width().ok() != Some(screen.width)
            || monitor.height().ok() != Some(screen.height)
        {
            return Err("显示器配置已变化，请重新选择屏幕".into());
        }
        let pixels = monitor
            .capture_image()
            .map_err(|e| format!("获取屏幕失败：{e}"))?;
        let image = xcap::image::DynamicImage::ImageRgba8(pixels);
        let maximum = match quality.preset.as_str() {
            "fluent" => 960,
            "clear" => 1920,
            _ => 1440,
        };
        let image = if image.width() > maximum || image.height() > maximum {
            image.resize(
                maximum,
                maximum,
                xcap::image::imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let mut rgb = image.to_rgb8();
        if quality.reduced_color {
            for byte in rgb.as_mut() {
                *byte = (*byte >> 3) << 3;
            }
        }
        let mut bytes = Vec::new();
        let compression = match quality.preset.as_str() {
            "fluent" => 45,
            "clear" => 80,
            _ => 65,
        };
        xcap::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, compression)
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
        if !interactive() {
            return Err("共享的电脑已锁屏".into());
        }
        Ok(bytes)
    }
    fn mouse(flags: u32, x: i32, y: i32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: x,
                    dy: y,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    fn keyboard(key: input::Key, down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: key.scan,
                    dwFlags: KEYEVENTF_SCANCODE
                        | if key.extended {
                            KEYEVENTF_EXTENDEDKEY
                        } else {
                            0
                        }
                        | if down { 0 } else { KEYEVENTF_KEYUP },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    fn button_flag(button: u8, down: bool) -> u32 {
        match (button, down) {
            (0, true) => MOUSEEVENTF_LEFTDOWN,
            (0, false) => MOUSEEVENTF_LEFTUP,
            (1, true) => MOUSEEVENTF_MIDDLEDOWN,
            (1, false) => MOUSEEVENTF_MIDDLEUP,
            (2, true) => MOUSEEVENTF_RIGHTDOWN,
            _ => MOUSEEVENTF_RIGHTUP,
        }
    }
    fn send(events: &[INPUT]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        // SAFETY: initialized INPUT records remain live for the synchronous Win32 call.
        let sent = unsafe {
            SendInput(
                events.len() as u32,
                events.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != events.len() {
            return Err("系统未接受远程输入；管理员窗口和安全桌面需要在本机操作".into());
        }
        Ok(())
    }
    pub fn release(held: &mut input::Held) -> Result<()> {
        if held.keys.is_empty() && held.buttons.is_empty() {
            return Ok(());
        }
        if !interactive() {
            return Err("等待桌面恢复后释放输入".into());
        }
        let mut events: Vec<_> = held.keys.iter().map(|key| keyboard(*key, false)).collect();
        events.extend(
            held.buttons
                .iter()
                .map(|button| mouse(button_flag(*button, false), 0, 0, 0)),
        );
        send(&events)?;
        held.keys.clear();
        held.buttons.clear();
        Ok(())
    }
    pub fn execute(screen: &Screen, event: &input::Event, held: &mut input::Held) -> Result<()> {
        event.validate()?;
        if !interactive() {
            return Err("共享的电脑已锁屏".into());
        }
        if matches!(event, input::Event::Release) {
            return release(held);
        }
        let mut events = Vec::with_capacity(2);
        match event {
            input::Event::Key { code, down } => {
                events.push(keyboard(input::key(code).ok_or("不支持的按键")?, *down))
            }
            input::Event::Move { x, y }
            | input::Event::Button { x, y, .. }
            | input::Event::Wheel { x, y, .. } => {
                let monitor = monitor(&screen.id)?;
                let width = monitor.width().map_err(|e| e.to_string())?;
                let height = monitor.height().map_err(|e| e.to_string())?;
                if width != screen.width || height != screen.height {
                    return Err("显示器配置变化，控制已停止".into());
                }
                let bounds = (
                    monitor.x().map_err(|e| e.to_string())?,
                    monitor.y().map_err(|e| e.to_string())?,
                    width,
                    height,
                );
                let desktop = unsafe {
                    (
                        GetSystemMetrics(SM_XVIRTUALSCREEN),
                        GetSystemMetrics(SM_YVIRTUALSCREEN),
                        GetSystemMetrics(SM_CXVIRTUALSCREEN),
                        GetSystemMetrics(SM_CYVIRTUALSCREEN),
                    )
                };
                let (dx, dy) = input::coordinates(*x, *y, bounds, desktop)?;
                events.push(mouse(
                    MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    dx,
                    dy,
                    0,
                ));
                match event {
                    input::Event::Button { button, down, .. } => {
                        events.push(mouse(button_flag(*button, *down), 0, 0, 0))
                    }
                    input::Event::Wheel {
                        delta, horizontal, ..
                    } => events.push(mouse(
                        if *horizontal {
                            MOUSEEVENTF_HWHEEL
                        } else {
                            MOUSEEVENTF_WHEEL
                        },
                        0,
                        0,
                        *delta as u32,
                    )),
                    _ => (),
                }
            }
            _ => (),
        }
        // Remember potential partial insertion so a failed SendInput still has matching releases.
        if let input::Event::Key { code, down: true } = event {
            held.keys.insert(input::key(code).unwrap());
        }
        if let input::Event::Button {
            button, down: true, ..
        } = event
        {
            held.buttons.insert(*button);
        }
        send(&events)
    }
}

pub fn interactive() -> bool {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    {
        return windows::interactive();
    }
    #[cfg(not(all(feature = "desktop", target_os = "windows")))]
    {
        true
    }
}
pub fn screens() -> Result<Vec<Screen>> {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    {
        return windows::screens();
    }
    #[cfg(not(all(feature = "desktop", target_os = "windows")))]
    {
        Err("当前平台请使用浏览器系统共享选择器".into())
    }
}
pub fn frame(screen: &Screen, quality: &Quality) -> Result<Vec<u8>> {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    {
        return windows::frame(screen, quality);
    }
    #[cfg(not(all(feature = "desktop", target_os = "windows")))]
    {
        let _ = (screen, quality);
        Err("当前平台不支持原生屏幕共享".into())
    }
}
pub fn execute(screen: &Screen, event: &input::Event, held: &mut input::Held) -> Result<()> {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    {
        return windows::execute(screen, event, held);
    }
    #[cfg(not(all(feature = "desktop", target_os = "windows")))]
    {
        let _ = (screen, event, held);
        Err("当前平台不支持本机输入控制".into())
    }
}
pub fn release(held: &mut input::Held) -> Result<()> {
    #[cfg(all(feature = "desktop", target_os = "windows"))]
    {
        return windows::release(held);
    }
    #[cfg(not(all(feature = "desktop", target_os = "windows")))]
    {
        held.keys.clear();
        held.buttons.clear();
        Ok(())
    }
}
