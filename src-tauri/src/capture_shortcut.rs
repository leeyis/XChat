use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[derive(Default)]
pub struct CaptureShortcutState {
    shortcut: Mutex<Option<String>>,
}

fn native_capture_shortcut(label: &str) -> Result<String, String> {
    let compact = label.replace(' ', "");
    let function_key = compact.to_ascii_uppercase().rfind('F').and_then(|index| {
        let suffix = &compact[index + 1..];
        suffix
            .parse::<u8>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| format!("F{number}"))
    });
    let key = compact
        .chars()
        .last()
        .filter(|character| character.is_ascii_alphanumeric())
        .ok_or_else(|| "快捷键必须包含字母或数字".to_string())?
        .to_ascii_uppercase();
    let lower = compact.to_ascii_lowercase();
    let either_primary = compact.contains("Ctrl/⌘")
        || compact.contains("⌘/Ctrl")
        || lower.contains("commandorcontrol")
        || lower.contains("cmdorctrl");
    let mut parts = Vec::new();
    if either_primary {
        parts.push("CommandOrControl");
    } else {
        if compact.contains('⌘') || lower.contains("command") || lower.contains("cmd") {
            parts.push("Command");
        }
        if compact.contains('⌃') || lower.contains("control") || lower.contains("ctrl") {
            parts.push("Control");
        }
    }
    if compact.contains('⌥') || lower.contains("option") || lower.contains("alt") {
        parts.push("Alt");
    }
    if compact.contains('⇧') || lower.contains("shift") {
        parts.push("Shift");
    }
    if parts.is_empty() && function_key.is_none() {
        return Err("快捷键必须包含至少一个修饰键".to_string());
    }
    let key = function_key.unwrap_or_else(|| key.to_string());
    parts.push(&key);
    Ok(parts.join("+"))
}

fn replace_registered_shortcut(
    current: &mut Option<String>,
    next: String,
    mut unregister: impl FnMut(&str) -> Result<(), String>,
    mut register: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    if current.as_deref() == Some(next.as_str()) {
        return Ok(());
    }
    let previous = current.take();
    if let Some(shortcut) = previous.as_deref() {
        if let Err(error) = unregister(shortcut) {
            *current = previous;
            return Err(error);
        }
    }
    if let Err(error) = register(&next) {
        if let Some(shortcut) = previous.as_deref() {
            if let Err(restore_error) = register(shortcut) {
                return Err(format!("{error}；恢复原快捷键失败: {restore_error}"));
            }
        }
        *current = previous;
        return Err(error);
    }
    *current = Some(next);
    Ok(())
}

pub fn register(app: &AppHandle, label: &str) -> Result<(), String> {
    let next = native_capture_shortcut(label)?;
    if next == "F3" || next == "Shift+F3" {
        return Err("F3 已用于贴图管理，请选择其他截图快捷键".to_string());
    }
    let state = app.state::<CaptureShortcutState>();
    let mut current = state
        .shortcut
        .lock()
        .map_err(|_| "全局快捷键状态不可用".to_string())?;
    replace_registered_shortcut(
        &mut current,
        next,
        |shortcut| {
            app.global_shortcut()
                .unregister(shortcut)
                .map_err(|error| format!("注销原快捷键失败: {error}"))
        },
        |shortcut| {
            app.global_shortcut()
                .register(shortcut)
                .map_err(|error| format!("注册全局快捷键失败: {error}"))
        },
    )
}

pub fn handle_shortcut(
    app: &AppHandle,
    shortcut: &tauri_plugin_global_shortcut::Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
    if event.state != ShortcutState::Pressed {
        return;
    }
    let workspace = shortcut.matches(Modifiers::empty(), Code::F3);
    let toggle_group = shortcut.matches(Modifiers::SHIFT, Code::F3);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = if toggle_group {
            crate::capture_editor::toggle_pin_group(&app).await
        } else if workspace {
            crate::capture_editor::open_workspace(&app)
        } else if crate::capture_editor::cancel_start(&app, None).unwrap_or(false) {
            Ok(())
        } else {
            crate::capture_editor::start(&app, None, None)
                .await
                .map(|_| ())
        };
        if let Err(error) = result {
            if error != "capture_cancelled" {
                eprintln!("[CaptureShortcut] {error}");
            }
        }
    });
}

pub fn register_workspace(app: &AppHandle) -> Result<(), String> {
    for shortcut in ["F3", "Shift+F3"] {
        if !app.global_shortcut().is_registered(shortcut) {
            app.global_shortcut()
                .register(shortcut)
                .map_err(|error| format!("注册贴图管理快捷键 {shortcut} 失败: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{native_capture_shortcut, replace_registered_shortcut};

    #[test]
    fn capture_shortcut_labels_convert_to_native_hotkeys() {
        assert_eq!(native_capture_shortcut("F1").unwrap(), "F1");
        assert_eq!(native_capture_shortcut("Ctrl+F12").unwrap(), "Control+F12");
        assert_eq!(
            native_capture_shortcut("Ctrl/⌘ ⇧ A").unwrap(),
            "CommandOrControl+Shift+A"
        );
        assert_eq!(native_capture_shortcut("⌘ ⌥ 7").unwrap(), "Command+Alt+7");
        assert_eq!(native_capture_shortcut("⌃ ⇧ X").unwrap(), "Control+Shift+X");
        assert_eq!(
            native_capture_shortcut("A").unwrap_err(),
            "快捷键必须包含至少一个修饰键"
        );
        assert_eq!(
            native_capture_shortcut("⇧").unwrap_err(),
            "快捷键必须包含字母或数字"
        );
    }

    #[test]
    fn failed_replacement_restores_the_previous_registration() {
        use std::cell::RefCell;

        let mut current = Some("CommandOrControl+Shift+A".to_string());
        let calls = RefCell::new(Vec::new());
        let result = replace_registered_shortcut(
            &mut current,
            "CommandOrControl+Shift+X".to_string(),
            |shortcut| {
                calls.borrow_mut().push(format!("unregister:{shortcut}"));
                Ok(())
            },
            |shortcut| {
                calls.borrow_mut().push(format!("register:{shortcut}"));
                if shortcut.ends_with('X') {
                    Err("快捷键已被占用".to_string())
                } else {
                    Ok(())
                }
            },
        );

        assert_eq!(result.unwrap_err(), "快捷键已被占用");
        assert_eq!(current.as_deref(), Some("CommandOrControl+Shift+A"));
        assert_eq!(
            calls.into_inner(),
            [
                "unregister:CommandOrControl+Shift+A",
                "register:CommandOrControl+Shift+X",
                "register:CommandOrControl+Shift+A",
            ]
        );
    }
}
