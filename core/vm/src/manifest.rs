//! Workspace manifest `rad.toml` (subset): `[network]` limits for remote module fetches.
//!
//! Parsed deterministically; unknown keys are ignored. Sizes may use `K`, `M`, `G` suffixes.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RadManifest {
    pub max_remote_module_bytes: usize,
    pub fetch_timeout: Duration,
}

impl Default for RadManifest {
    fn default() -> Self {
        Self {
            max_remote_module_bytes: 2 * 1024 * 1024,
            fetch_timeout: Duration::from_secs(5),
        }
    }
}

fn parse_byte_size(s: &str) -> Result<usize, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".to_string());
    }
    let upper = s.to_ascii_uppercase();
    let (num_part, mult) = if let Some(rest) = upper.strip_suffix('K') {
        (rest.trim(), 1024usize)
    } else if let Some(rest) = upper.strip_suffix('M') {
        (rest.trim(), 1024 * 1024)
    } else if let Some(rest) = upper.strip_suffix('G') {
        (rest.trim(), 1024 * 1024 * 1024)
    } else {
        (s, 1usize)
    };
    let n: u64 = num_part
        .parse()
        .map_err(|_| format!("invalid integer in size: {s}"))?;
    let bytes = n
        .checked_mul(mult as u64)
        .ok_or_else(|| "size overflow".to_string())?;
    usize::try_from(bytes).map_err(|_| "size too large for this platform".to_string())
}

fn parse_u64(s: &str) -> Result<u64, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("invalid integer: {s}"))
}

/// Parse `rad.toml` content. Returns default manifest on empty / no `[network]` section.
pub fn parse_rad_toml(content: &str) -> Result<RadManifest, String> {
    let mut manifest = RadManifest::default();
    let mut in_network = false;

    for raw in content.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[network]" {
            in_network = true;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_network = false;
            continue;
        }
        if !in_network {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(format!("expected key = value in [network]: {line}"));
        }
        let key = parts[0].trim();
        let val = parts[1].trim().trim_matches('"').trim_matches('\'');

        match key {
            "max_module_size" => {
                manifest.max_remote_module_bytes = parse_byte_size(val)?;
            }
            "fetch_timeout_secs" | "fetch_timeout" => {
                let secs = parse_u64(val)?;
                manifest.fetch_timeout = Duration::from_secs(secs);
            }
            _ => {}
        }
    }

    if manifest.max_remote_module_bytes == 0 {
        return Err("max_module_size must be > 0".to_string());
    }
    if manifest.fetch_timeout.is_zero() {
        return Err("fetch timeout must be > 0".to_string());
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_network_section() {
        let m = parse_rad_toml("# empty\n").unwrap();
        assert_eq!(m.max_remote_module_bytes, 2 * 1024 * 1024);
        assert_eq!(m.fetch_timeout.as_secs(), 5);
    }

    #[test]
    fn parses_network_block() {
        let src = r#"
[network]
max_module_size = 4M
fetch_timeout_secs = 12
"#;
        let m = parse_rad_toml(src).unwrap();
        assert_eq!(m.max_remote_module_bytes, 4 * 1024 * 1024);
        assert_eq!(m.fetch_timeout.as_secs(), 12);
    }
}
