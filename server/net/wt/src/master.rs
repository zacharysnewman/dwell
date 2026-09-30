//! Registration with the master server (ARCHITECTURE.md §10.1, §10.3, Phase 5d).
//!
//! A background thread posts the server's heartbeats to `POST /v1/servers` and, on shutdown,
//! `POST /v1/servers/leave`, each signed with the server's Ed25519 key in the master's format:
//! the signature covers `"dwell-master-v1\n" ‖ method ‖ "\n" ‖ path ‖ "\n" ‖ time ‖ "\n" ‖
//! SHA-256(body)` and is sent as `X-Dwell-Key`, `X-Dwell-Time` and `X-Dwell-Signature` (hex).
//! The host builds each heartbeat's JSON; the latest one queued wins. Nothing blocks the tick.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::digest::{SHA256, digest};
use ring::signature::Ed25519KeyPair;

const SIGNATURE_CONTEXT: &str = "dwell-master-v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const LEAVE_TIMEOUT: Duration = Duration::from_secs(3);

/// What the host sees of the registration.
#[derive(Default, Clone)]
pub struct Status {
    /// The join code as shown ("KQ7-XM4"), once the master has answered.
    pub display: String,
    /// The last failure (empty after a successful heartbeat).
    pub error: String,
}

enum Job {
    Heartbeat(String),
    Stop { leave: bool, done: Sender<()> },
}

pub struct Master {
    jobs: Sender<Job>,
    status: Arc<Mutex<Status>>,
    thread: Option<JoinHandle<()>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bytes a request's signature covers (the master's `signingMessage`).
pub fn signing_message(method: &str, path: &str, time_ms: u64, body: &[u8]) -> Vec<u8> {
    let mut message = format!(
        "{SIGNATURE_CONTEXT}\n{}\n{path}\n{time_ms}\n",
        method.to_uppercase()
    )
    .into_bytes();
    message.extend_from_slice(digest(&SHA256, body).as_ref());
    message
}

/// The signature headers for a request, signed with `key`.
pub fn signature_headers(
    key: &Ed25519KeyPair,
    method: &str,
    path: &str,
    time_ms: u64,
    body: &[u8],
) -> [(&'static str, String); 3] {
    use ring::signature::KeyPair;
    let signature = key.sign(&signing_message(method, path, time_ms, body));
    [
        ("x-dwell-key", hex(key.public_key().as_ref())),
        ("x-dwell-time", time_ms.to_string()),
        ("x-dwell-signature", hex(signature.as_ref())),
    ]
}

/// A string field of a flat JSON object (enough for the master's answers), unescaped naively.
fn json_string_field(json: &str, field: &str) -> Option<String> {
    let at = json.find(&format!("\"{field}\""))?;
    let rest = json[at + field.len() + 2..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let rest = rest.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

impl Master {
    /// Starts the registration thread for the master at `base_url` (no trailing slash needed).
    pub fn start(base_url: &str, seed: [u8; 32]) -> Result<Master, String> {
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|e| e.to_string())?;
        let base = base_url.trim_end_matches('/').to_string();
        if !(base.starts_with("https://") || base.starts_with("http://")) {
            return Err(format!("master URL {base:?} is not http(s)"));
        }
        let (jobs, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = status.clone();
        let thread = std::thread::Builder::new()
            .name("dwell-master".into())
            .spawn(move || run(base, key, rx, shared))
            .map_err(|e| e.to_string())?;
        Ok(Master {
            jobs,
            status,
            thread: Some(thread),
        })
    }

    /// Queues a heartbeat (JSON body); if several are waiting, only the latest is sent.
    pub fn heartbeat(&self, body: String) {
        let _ = self.jobs.send(Job::Heartbeat(body));
    }

    pub fn status(&self) -> Status {
        self.status.lock().expect("status lock").clone()
    }

    /// Stops the thread; with `leave`, first tells the master (waiting at most a few seconds).
    pub fn stop(mut self, leave: bool) {
        let (done, wait) = mpsc::channel();
        if self.jobs.send(Job::Stop { leave, done }).is_ok() {
            let _ = wait.recv_timeout(LEAVE_TIMEOUT + Duration::from_secs(1));
        }
        // The thread ends after the leave (or its timeout); a hung request is not waited for.
        drop(self.thread.take());
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn post(
    agent: &ureq::Agent,
    base: &str,
    key: &Ed25519KeyPair,
    path: &str,
    body: &str,
) -> Result<String, String> {
    let mut request = agent
        .post(format!("{base}{path}"))
        .header("content-type", "application/json");
    for (name, value) in signature_headers(key, "POST", path, now_ms(), body.as_bytes()) {
        request = request.header(name, value);
    }
    let mut response = request.send(body.as_bytes()).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    if (200..300).contains(&status) {
        Ok(text)
    } else {
        let message = json_string_field(&text, "message").unwrap_or(text);
        Err(format!("HTTP {status}: {message}"))
    }
}

fn run(base: String, key: Ed25519KeyPair, jobs: Receiver<Job>, status: Arc<Mutex<Status>>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    loop {
        let job = match jobs.recv_timeout(Duration::from_secs(3600)) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut next = Some(job);
        // Only the latest heartbeat matters; a stop wins over any.
        while let Ok(later) = jobs.try_recv() {
            if matches!(next, Some(Job::Stop { .. })) {
                break;
            }
            next = Some(later);
        }
        match next {
            Some(Job::Heartbeat(body)) => {
                let result = post(&agent, &base, &key, "/v1/servers", &body);
                let mut s = status.lock().expect("status lock");
                match result {
                    Ok(text) => {
                        if let Some(display) = json_string_field(&text, "display") {
                            s.display = display;
                        }
                        s.error.clear();
                    }
                    Err(e) => s.error = e,
                }
            }
            Some(Job::Stop { leave, done }) => {
                if leave {
                    let agent: ureq::Agent = ureq::Agent::config_builder()
                        .timeout_global(Some(LEAVE_TIMEOUT))
                        .http_status_as_error(false)
                        .build()
                        .into();
                    let _ = post(&agent, &base, &key, "/v1/servers/leave", "{}");
                }
                let _ = done.send(());
                return;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn signs_like_the_client_and_the_master() {
        // shared/master/vectors.json: the format both the client and the master check.
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../shared/master/vectors.json"
        ))
        .expect("vectors");
        let vectors: serde_json::Value = serde_json::from_str(&text).unwrap();
        // The PKCS#8 key ends with its 32-byte seed.
        let pkcs8 = from_hex(vectors["pkcs8"].as_str().unwrap());
        let seed: [u8; 32] = pkcs8[pkcs8.len() - 32..].try_into().unwrap();
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        for case in cases {
            let method = case["method"].as_str().unwrap();
            let path = case["path"].as_str().unwrap();
            let body = case["body"].as_str().unwrap().as_bytes();
            let time = case["time"].as_u64().unwrap();
            let message = signing_message(method, path, time, body);
            assert_eq!(hex(&message), case["message"].as_str().unwrap(), "{path}");
            let headers = signature_headers(&key, method, path, time, body);
            assert_eq!(headers[0].1, vectors["publicKey"].as_str().unwrap());
            assert_eq!(headers[1].1, time.to_string());
            assert_eq!(headers[2].1, case["signature"].as_str().unwrap(), "{path}");
        }
        assert!(cases.len() >= 2);
    }

    #[test]
    fn reads_string_fields() {
        let json = r#"{"code":"KQ7XM4", "display" : "KQ7-XM4","heartbeatS":30}"#;
        assert_eq!(
            json_string_field(json, "display").as_deref(),
            Some("KQ7-XM4")
        );
        assert_eq!(json_string_field(json, "code").as_deref(), Some("KQ7XM4"));
        assert_eq!(json_string_field(json, "missing"), None);
    }
}
