use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::path::Path;

/// Keep startup registration local to this computer, outside the shared database.
pub fn launcher(app_name: &str, executable: &Path) -> Result<AutoLaunch, String> {
    let path = executable
        .to_str()
        .ok_or_else(|| "The startup executable path is not valid Unicode".to_string())?;
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(app_name);

    // auto-launch writes these paths directly into a command line / Exec field.
    #[cfg(target_os = "windows")]
    builder.set_app_path(&format!("\"{path}\""));

    #[cfg(target_os = "linux")]
    {
        let mut escaped = String::from("\"");
        for character in path.chars() {
            match character {
                '\\' => escaped.push_str("\\\\\\\\"),
                '"' | '`' | '$' => {
                    escaped.push_str("\\\\");
                    escaped.push(character);
                }
                '%' => escaped.push_str("%%"),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                _ => escaped.push(character),
            }
        }
        escaped.push('"');
        builder.set_app_path(&escaped);
    }

    #[cfg(target_os = "macos")]
    builder.set_app_path(path).set_use_launch_agent(true);

    builder.build().map_err(|error| error.to_string())
}

pub fn set_enabled(launcher: &AutoLaunch, enabled: bool) -> Result<(), String> {
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    match result {
        // Removing an already-absent Windows Run value is a successful disable.
        Err(auto_launch::Error::Io(error))
            if !enabled && error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(())
        }
        result => result.map_err(|error| error.to_string()),
    }
}
