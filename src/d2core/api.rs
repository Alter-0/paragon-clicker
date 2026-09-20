use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const ENV: &str = "diablocore-4gkv4qjs9c6a0b40";
pub const APP_SIGN: &str = "diablocore";
pub const APP_ACCESS_KEY_ID: i64 = 1;
pub const APP_ACCESS_KEY: &str = "ed6fe96e6ca08acf392d360094a58477";
pub const PARAGON_VERSION: &str = "71566";
pub const PARAGON_LOCALE: &str = "zhCN";
pub const PARAGON_REVISION: &str = "26";

static DB_CACHE: Lazy<Mutex<Option<Arc<Value>>>> = Lazy::new(|| Mutex::new(None));

pub fn base64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

pub fn create_sign(payload: &Value, secret: &str) -> String {
    let header = json!({"alg": "HS256", "typ": "JWT"});
    let encoded_header = base64url(serde_json::to_string(&header).unwrap().as_bytes());
    let encoded_payload = base64url(serde_json::to_string(payload).unwrap().as_bytes());
    let message = format!("{encoded_header}.{encoded_payload}");

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    let signature = mac.finalize().into_bytes();
    let encoded_sig = base64url(&signature);

    format!("{message}.{encoded_sig}")
}

pub fn invoke_cloud_function(function_name: &str, request_data: &Value) -> Result<Value, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();

    let sign = create_sign(
        &json!({
            "data": {},
            "timestamp": timestamp,
            "appAccessKeyId": APP_ACCESS_KEY_ID,
            "appSign": APP_SIGN,
        }),
        APP_ACCESS_KEY,
    );

    let payload = json!({
        "action": "functions.invokeFunction",
        "dataVersion": "2020-01-10",
        "env": ENV,
        "function_name": function_name,
        "request_data": serde_json::to_string(request_data).map_err(|e| e.to_string())?,
    });

    let body_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(&body_bytes);
    let md5_digest = hasher.finalize();
    let seqid = format!("{:x}", md5_digest)[..16].to_string();

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://tcb-api.tencentcloudapi.com/web?env={ENV}");
    let resp = client
        .post(&url)
        .header("content-type", "application/json;charset=UTF-8")
        .header(
            "X-TCB-App-Source",
            format!("timestamp={timestamp};appAccessKeyId={APP_ACCESS_KEY_ID};appSign={APP_SIGN};sign={sign}"),
        )
        .header("x-seqid", seqid)
        .header("X-SDK-Version", "@cloudbase/js-sdk/python-local")
        .body(body_bytes)
        .send()
        .map_err(|e| format!("Network request failed: {e}"))?;

    let resp_json: Value = resp.json().map_err(|e| format!("Failed to parse JSON response: {e}"))?;
    let response_data_str = resp_json
        .get("data")
        .and_then(|d| d.get("response_data"))
        .and_then(|r| r.as_str())
        .ok_or_else(|| format!("Cloud function returned invalid response: {resp_json}"))?;

    serde_json::from_str(response_data_str).map_err(|e| format!("Failed to parse response_data: {e}"))
}

pub fn query_plan(bd: &str) -> Result<Value, String> {
    invoke_cloud_function(
        "function-planner-queryplan",
        &json!({"bd": bd, "enableVariant": true}),
    )
}

pub fn fetch_paragon_db(version: Option<&str>, locale: Option<&str>) -> Result<Arc<Value>, String> {
    let mut cache = DB_CACHE.lock().unwrap();
    if let Some(ref db) = *cache {
        return Ok(Arc::clone(db));
    }

    let ver = version.unwrap_or(PARAGON_VERSION);
    let loc = locale.unwrap_or(PARAGON_LOCALE);
    let url = format!("https://cloudstorage.d2core.com/data/d4/{ver}/paragon_{loc}.json?env=prod&v={PARAGON_REVISION}");

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url).send().map_err(|e| format!("Failed to fetch paragon database: {e}"))?;
    let db_val: Value = resp.json().map_err(|e| format!("Failed to parse paragon database JSON: {e}"))?;
    let arc_db = Arc::new(db_val);
    *cache = Some(Arc::clone(&arc_db));
    Ok(arc_db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sign() {
        let payload = json!({"test": 123});
        let sign = create_sign(&payload, "test_secret");
        assert!(sign.contains('.'));
        let parts: Vec<&str> = sign.split('.').collect();
        assert_eq!(parts.len(), 3);
    }
}
