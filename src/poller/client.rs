use super::config::PollerConfig;
use super::types::{PendingResponse, ReceiptMessage};

pub async fn fetch_pending(
    client: &reqwest::Client,
    config: &PollerConfig,
) -> Result<Vec<ReceiptMessage>, String> {
    let url = format!("{}/api/receipt_messages/pending", config.base_url);

    let resp = client
        .get(&url)
        .query(&[("auth_token", &config.auth_token)])
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("Auth failed — check RECEIPT_PRINTER_API_TOKEN".into());
    }
    if !status.is_success() {
        return Err(format!("Unexpected status {status}"));
    }

    let data: PendingResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    Ok(data.messages)
}

pub async fn mark_printed(
    client: &reqwest::Client,
    config: &PollerConfig,
    message_id: i64,
) -> Result<(), String> {
    let url = format!(
        "{}/api/receipt_messages/{}/printed",
        config.base_url, message_id
    );

    client
        .post(&url)
        .query(&[("auth_token", &config.auth_token)])
        .send()
        .await
        .map_err(|e| format!("Failed to mark printed: {e}"))?;

    Ok(())
}

/// Download image bytes from a relative or absolute URL.
pub async fn download_image(
    client: &reqwest::Client,
    config: &PollerConfig,
    image_url: &str,
) -> Result<Vec<u8>, String> {
    // If the URL is relative, prepend base_url
    let full_url = if image_url.starts_with("http") {
        image_url.to_string()
    } else {
        format!("{}{}", config.base_url, image_url)
    };

    let resp = client
        .get(&full_url)
        .query(&[("auth_token", &config.auth_token)])
        .send()
        .await
        .map_err(|e| format!("Image download failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Image download returned {}", resp.status()));
    }

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("Failed to read image bytes: {e}"))
}

pub async fn mark_failed(
    client: &reqwest::Client,
    config: &PollerConfig,
    message_id: i64,
) -> Result<(), String> {
    let url = format!(
        "{}/api/receipt_messages/{}/failed",
        config.base_url, message_id
    );

    client
        .post(&url)
        .query(&[("auth_token", &config.auth_token)])
        .send()
        .await
        .map_err(|e| format!("Failed to mark failed: {e}"))?;

    Ok(())
}
