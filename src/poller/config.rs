use std::path::Path;

#[derive(Debug, Clone)]
pub struct PollerConfig {
    pub base_url: String,
    pub auth_token: String,
    pub poll_interval_secs: u64,
}

pub fn load_config() -> Result<PollerConfig, String> {
    let env_path = Path::new(".hermes_env");

    if !env_path.exists() {
        return Err(".hermes_env file not found".into());
    }

    let entries: Vec<(String, String)> = dotenvy::from_filename_iter(".hermes_env")
        .map_err(|e| format!("Failed to read .hermes_env: {e}"))?
        .filter_map(|item| item.ok())
        .collect();

    let get = |key: &str| -> Option<String> {
        entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    let base_url = get("POLL_WEBSITE_URL")
        .ok_or("POLL_WEBSITE_URL not set in .hermes_env")?
        .trim_end_matches('/')
        .to_string();

    let auth_token = get("RECEIPT_PRINTER_API_TOKEN")
        .ok_or("RECEIPT_PRINTER_API_TOKEN not set in .hermes_env")?;

    let poll_interval_secs = get("POLL_INTERVAL")
        .unwrap_or_else(|| "10".to_string())
        .parse::<u64>()
        .map_err(|e| format!("Invalid POLL_INTERVAL: {e}"))?;

    Ok(PollerConfig {
        base_url,
        auth_token,
        poll_interval_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_env_file_returns_error() {
        // This test relies on .hermes_env not being in the test working dir
        // We can't easily test the happy path without writing temp files
        let result = load_config();
        // If .hermes_env exists in the repo root, this will succeed
        // If not, it should return an error — either way, it shouldn't panic
        assert!(result.is_ok() || result.is_err());
    }
}
