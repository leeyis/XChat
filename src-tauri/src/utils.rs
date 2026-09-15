use rand::Rng;

const RANDOM_NAME_ADJECTIVES: &[&str] =
    &["Fast", "Swift", "Quiet", "Happy", "Brave", "Cool", "Lazy"];
const RANDOM_NAME_ANIMALS: &[&str] = &["Crab", "Panda", "Tiger", "Fox", "Whale", "Eagle", "Cat"];

pub fn generate_random_name() -> String {
    let mut rng = rand::thread_rng();
    let adj = RANDOM_NAME_ADJECTIVES[rng.gen_range(0..RANDOM_NAME_ADJECTIVES.len())];
    let animal = RANDOM_NAME_ANIMALS[rng.gen_range(0..RANDOM_NAME_ANIMALS.len())];
    let num: u32 = rng.gen_range(100..999);

    format!("{}-{}-{}", adj, animal, num)
}

pub fn is_legacy_generated_name(name: &str) -> bool {
    let mut parts = name.split('-');
    let (Some(adjective), Some(animal), Some(number), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    RANDOM_NAME_ADJECTIVES.contains(&adjective)
        && RANDOM_NAME_ANIMALS.contains(&animal)
        && number.len() == 3
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number
            .parse::<u16>()
            .is_ok_and(|value| (100..999).contains(&value))
}

/// 过滤掉拿不到有意义名字的情况。"localhost" 是 Android 上 gethostname()
/// 的固定返回值，等于没拿到。
#[cfg(target_os = "android")]
fn tidy_device_name(value: String) -> Option<String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() || trimmed == "localhost" {
        None
    } else {
        Some(trimmed)
    }
}

/// 读系统设置里的 device_name（用户在「设置 → 关于手机」里改的那个名字）。
#[cfg(target_os = "android")]
fn settings_device_name(
    env: &mut jni::JNIEnv,
    activity: &jni::objects::JObject,
) -> Option<String> {
    use jni::objects::{JString, JValue};

    let resolver = env
        .call_method(
            activity,
            "getContentResolver",
            "()Landroid/content/ContentResolver;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    let key = env.new_string("device_name").ok()?;
    let value = env
        .call_static_method(
            "android/provider/Settings$Global",
            "getString",
            "(Landroid/content/ContentResolver;Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&resolver), JValue::Object(&key)],
        )
        .ok()?
        .l()
        .ok()?;
    if value.is_null() {
        return None;
    }
    let text: String = env.get_string(&JString::from(value)).ok()?.into();
    tidy_device_name(text)
}

/// Android 上 gethostname() 固定返回 "localhost"，拿不到有意义的设备名，
/// 所以先读系统设置里的 device_name，再退回 Build.MODEL（如 "ONEPLUS A6000"）。
/// 任何一步失败都返回 None，由调用方回退到原逻辑。
#[cfg(target_os = "android")]
fn android_device_name() -> Option<String> {
    use jni::objects::{JObject, JString};

    let context = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(context.context().cast()) };

    if let Some(name) = settings_device_name(&mut env, &activity) {
        return Some(name);
    }

    let model = env
        .get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")
        .ok()?
        .l()
        .ok()?;
    if model.is_null() {
        return None;
    }
    let text: String = env.get_string(&JString::from(model)).ok()?.into();
    tidy_device_name(text)
}

pub fn machine_name() -> String {
    #[cfg(target_os = "android")]
    if let Some(name) = android_device_name() {
        return name;
    }

    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return name;
            }
        }
    }

    sysinfo::System::host_name()
        .map(|name| {
            name.strip_suffix(".local")
                .unwrap_or(&name)
                .trim()
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(generate_random_name)
}

#[cfg(test)]
mod tests {
    use super::is_legacy_generated_name;

    #[test]
    fn legacy_generated_name_match_is_exact() {
        assert!(is_legacy_generated_name("Happy-Fox-662"));
        assert!(!is_legacy_generated_name("Happy-Fox-999"));
        assert!(!is_legacy_generated_name("Custom-Fox-662"));
        assert!(!is_legacy_generated_name("Happy-Fox-662-extra"));
    }
}
