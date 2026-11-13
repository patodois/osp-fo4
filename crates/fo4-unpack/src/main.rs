#![forbid(unsafe_code)]
fn main() {
    if let Err(error) = start_memory_watchdog() {
        eprintln!("{error}");
        std::process::exit(3);
    }
    let code = match fo4_unpack::Options::parse(std::env::args().skip(1))
        .and_then(|o| fo4_unpack::run(&o))
    {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(code as i32);
}

// Opt-in operational guard for the read-only real-corpus run on macOS.
fn memory_percentage(output: &str) -> Result<u8, String> {
    let line = output
        .lines()
        .find_map(|line| line.strip_prefix("System-wide memory free percentage: "))
        .ok_or_else(|| "MemoryMonitorUnavailable: missing free percentage".to_string())?;
    line.trim_end_matches('%')
        .parse::<u8>()
        .ok()
        .filter(|n| *n <= 100)
        .ok_or_else(|| "MemoryMonitorUnavailable: invalid free percentage".to_string())
}
fn check_memory(floor: u8) -> Result<(), String> {
    let result = std::process::Command::new("/usr/bin/memory_pressure")
        .arg("-Q")
        .output()
        .map_err(|e| format!("MemoryMonitorUnavailable: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "MemoryMonitorUnavailable: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let free = memory_percentage(&String::from_utf8_lossy(&result.stdout))?;
    if free < floor {
        return Err(format!(
            "MemoryFloorReached: {free}% available < {floor}%; stopping read-only run"
        ));
    }
    Ok(())
}
fn start_memory_watchdog() -> Result<(), String> {
    let Some(value) = std::env::var_os("FO4_MIN_FREE_PERCENT") else {
        return Ok(());
    };
    let floor = value
        .to_str()
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|n| (1..=100).contains(n))
        .ok_or_else(|| "InvalidMemoryFloor: expected 1..100".to_string())?;
    check_memory(floor)?;
    std::thread::Builder::new()
        .name("memory-floor".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Err(error) = check_memory(floor) {
                eprintln!("{error}");
                std::process::exit(3);
            }
        })
        .map_err(|e| format!("MemoryMonitorUnavailable: {e}"))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn parses_memory_monitor_without_assuming_missing_values() {
        assert_eq!(
            super::memory_percentage(
                "The system has pages.\nSystem-wide memory free percentage: 44%\n"
            )
            .unwrap(),
            44
        );
        for text in [
            "",
            "System-wide memory free percentage: nope%",
            "System-wide memory free percentage: 101%",
        ] {
            assert!(super::memory_percentage(text).is_err());
        }
    }
}
