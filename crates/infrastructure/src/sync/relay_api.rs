//! The relay's HTTP API (`relay/sync`), independent of any HTTP client:
//!
//! - `PUT    {base}/v1/groups/{group}/devices/{device}`  body: the blob
//! - `GET    {base}/v1/groups/{group}`  → `{"devices":[{"id","data"}]}` (base64)
//! - `DELETE {base}/v1/groups/{group}/devices/{device}`
//!
//! with `Authorization: Bearer {token}`.

use application::ports::SyncError;
use data_encoding::BASE64;
use serde::Deserialize;

pub fn group_url(base: &str, group: &str) -> String {
    format!("{}/v1/groups/{group}", base.trim_end_matches('/'))
}

pub fn device_url(base: &str, group: &str, device: &str) -> String {
    format!("{}/devices/{device}", group_url(base, group))
}

#[derive(Deserialize)]
struct GroupDto {
    devices: Vec<BlobDto>,
}

#[derive(Deserialize)]
struct BlobDto {
    id: String,
    data: String,
}

/// The blobs of a `GET` answer, by device id. Undecodable ones are skipped.
pub fn decode_group(json: &str) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let group: GroupDto =
        serde_json::from_str(json).map_err(|e| SyncError(format!("relay answer: {e}")))?;
    Ok(group
        .devices
        .into_iter()
        .filter_map(|b| Some((b.id, BASE64.decode(b.data.as_bytes()).ok()?)))
        .collect())
}

/// The relay's `{"error": …}` message, else the raw body.
pub fn error_message(status: u16, body: &str) -> SyncError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| body.to_owned());
    SyncError(format!("relay: {status} {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_answers() {
        assert_eq!(
            group_url("https://r.test/", "g"),
            "https://r.test/v1/groups/g"
        );
        assert_eq!(
            device_url("https://r.test", "g", "d"),
            "https://r.test/v1/groups/g/devices/d"
        );
        let blobs = decode_group(r#"{"devices":[{"id":"a","data":"AQI="},{"id":"b","data":"!"}]}"#)
            .unwrap();
        assert_eq!(blobs, vec![("a".to_owned(), vec![1, 2])]);
        assert!(decode_group("nope").is_err());
        assert_eq!(
            error_message(403, r#"{"error":"Forbidden."}"#).0,
            "relay: 403 Forbidden."
        );
        assert_eq!(error_message(500, "boom").0, "relay: 500 boom");
    }
}
