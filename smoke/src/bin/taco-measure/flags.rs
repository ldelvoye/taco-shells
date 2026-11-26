const NAMES: [&str; 5] = ["terminals", "full", "flooding", "flood-seconds", "samples"];

pub struct Flags {
    pub terminals: usize,
    pub full: usize,
    pub flooding: usize,
    pub flood_seconds: u64,
    pub samples: usize,
}

pub fn parse(arguments: &[String]) -> Result<Flags, String> {
    let mut terminals = 24;
    let mut full = 8;
    let mut flooding = 4;
    let mut flood_seconds = 10;
    let mut samples = 50;

    for argument in arguments {
        let Some(rest) = argument.strip_prefix("--") else {
            return Err(format!(
                "unrecognised argument {argument}; flags look like --terminals=24"
            ));
        };
        let Some((name, text)) = rest.split_once('=') else {
            return Err(format!(
                "unrecognised argument {argument}; flags look like --terminals=24"
            ));
        };
        if !NAMES.contains(&name) {
            let choices = NAMES.join(", ");
            return Err(format!("unknown flag --{name}; pick from {choices}"));
        }
        let value = whole_number(name, text)?;
        match name {
            "terminals" => terminals = value as usize,
            "full" => full = value as usize,
            "flooding" => flooding = value as usize,
            "flood-seconds" => flood_seconds = value,
            _ => samples = value as usize,
        }
    }

    if flooding > full {
        return Err(format!(
            "--flooding ({flooding}) must not be more than --full ({full})"
        ));
    }
    if full + 2 > terminals {
        return Err(format!(
            "--full ({full}) plus 2 must not be more than --terminals ({terminals})"
        ));
    }
    Ok(Flags {
        terminals,
        full,
        flooding,
        flood_seconds,
        samples,
    })
}

pub fn describe(flags: &Flags) -> String {
    format!(
        "--terminals={} --full={} --flooding={} --flood-seconds={} --samples={}",
        flags.terminals, flags.full, flags.flooding, flags.flood_seconds, flags.samples
    )
}

fn whole_number(name: &str, text: &str) -> Result<u64, String> {
    let message = format!("--{name} must be a whole number of at least 1, got \"{text}\"");
    let all_digits = !text.is_empty() && text.chars().all(|character| character.is_ascii_digit());
    if !all_digits {
        return Err(message);
    }
    let value: u64 = text.parse().map_err(|_| message.clone())?;
    if value < 1 {
        return Err(message);
    }
    Ok(value)
}
