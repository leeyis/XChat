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

pub fn machine_name() -> String {
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
