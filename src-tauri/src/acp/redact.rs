//! Credential redaction and diagnostic payload bounding for ACP events/logs.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";

/// Default max bytes retained for a single diagnostic payload after redaction.
pub const DEFAULT_DIAGNOSTIC_MAX_BYTES: usize = 64 * 1024;

/// Metadata written when a diagnostic payload is truncated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TruncationMeta {
    pub truncated: bool,
    pub original_bytes: usize,
    pub preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_bytes: Option<usize>,
}

fn secret_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(token|secret|password|passwd|api[_-]?key|private[_-]?key|authorization|auth|credential|cookie|session[_-]?id|access[_-]?key|client[_-]?secret)",
        )
        .unwrap()
    })
}

fn patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        vec![
            // Authorization / Bearer headers
            Regex::new(r"(?i)(authorization\s*[:=]\s*)(bearer\s+)?\S+").unwrap(),
            Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9\-\._~\+/]+=*").unwrap(),
            // Common key=value secret assignments
            Regex::new(
                r"(?i)(api[_-]?key|secret|password|passwd|token|private[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret)\s*[:=]\s*\S+",
            )
            .unwrap(),
            Regex::new(r"(?i)(x-api-key\s*[:=]\s*)\S+").unwrap(),
            // URL query credential params
            Regex::new(r"(?i)([?&](?:access_token|refresh_token|id_token|token|api_key|apikey|password|secret|key)=)([^&\s#]+)").unwrap(),
            // URLs with embedded credentials: scheme://user:pass@host
            Regex::new(r"([a-zA-Z][a-zA-Z0-9+.\-]*://)([^/\s:@]+):([^@/\s]+)@").unwrap(),
            // Well-known token prefixes
            Regex::new(r"\bsk-[A-Za-z0-9]{10,}\b").unwrap(),
            Regex::new(r"\bghp_[A-Za-z0-9]{20,}\b").unwrap(),
            Regex::new(r"\bgho_[A-Za-z0-9]{20,}\b").unwrap(),
            Regex::new(r"\bxox[baprs]-[A-Za-z0-9\-]{10,}\b").unwrap(),
            // ENV-style: export FOO_TOKEN=value / FOO_API_KEY=value
            Regex::new(
                r"(?i)\b([A-Z][A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|PRIVATE_KEY|ACCESS_KEY)[A-Z0-9_]*)\s*=\s*(\S+)",
            )
            .unwrap(),
            // Long high-entropy tokens (base64/hex-ish, 32+ chars) with surrounding keyword context
            Regex::new(
                r"(?i)((?:token|secret|key|password|authorization|bearer)[^A-Za-z0-9]{0,8})([A-Za-z0-9+/_\-]{32,}={0,2})",
            )
            .unwrap(),
        ]
    })
}

fn is_sensitive_key(key: &str) -> bool {
    secret_key_re().is_match(key)
}

/// Redact secrets from a free-form string.
pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for re in patterns() {
        out = re
            .replace_all(&out, |caps: &regex::Captures| {
                // URL with user:pass@host → keep scheme, redact credentials
                if caps.len() >= 4
                    && caps
                        .get(0)
                        .map(|m| m.as_str().contains("://"))
                        .unwrap_or(false)
                {
                    if let (Some(scheme), Some(_user), Some(_pass)) =
                        (caps.get(1), caps.get(2), caps.get(3))
                    {
                        if scheme.as_str().contains("://") {
                            return format!("{}{REDACTED}:{REDACTED}@", scheme.as_str());
                        }
                    }
                }
                // Query param style group1=prefix group2=value
                if caps.len() >= 3 {
                    if let Some(prefix) = caps.get(1) {
                        let p = prefix.as_str();
                        // ENV FOO_TOKEN=value → keep name=
                        if p.ends_with('=') || p.contains('=') {
                            // For ([?&]key=)(value) keep prefix
                            if p.starts_with('?') || p.starts_with('&') {
                                return format!("{p}{REDACTED}");
                            }
                        }
                        // Authorization: / api_key=
                        if p.chars().any(|c| c == ':' || c == '=')
                            || p.to_ascii_lowercase().contains("authorization")
                            || p.to_ascii_lowercase().contains("bearer")
                            || p.to_ascii_lowercase().contains("token")
                            || p.to_ascii_lowercase().contains("secret")
                            || p.to_ascii_lowercase().contains("key")
                            || p.to_ascii_lowercase().contains("password")
                        {
                            // ENV NAME=value has group1=NAME, group2=value
                            if caps.name("unused").is_none()
                                && caps.len() == 3
                                && !p.contains(':')
                                && !p.contains('=')
                                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                            {
                                return format!("{p}={REDACTED}");
                            }
                            return format!("{p}{REDACTED}");
                        }
                    }
                }
                if let Some(p) = caps.get(1) {
                    let s = p.as_str();
                    if s.ends_with('=') || s.ends_with(':') || s.ends_with(' ') {
                        return format!("{s}{REDACTED}");
                    }
                    return format!("{s}{REDACTED}");
                }
                REDACTED.to_string()
            })
            .into_owned();
    }
    // Standalone high-entropy secrets (32+ chars, mixed classes) when not already redacted.
    out = redact_high_entropy_tokens(&out);
    out
}

fn redact_high_entropy_tokens(input: &str) -> String {
    // Replace long tokens that look like API keys when adjacent context implies secrets,
    // and also bare sk-/ghp- already handled. Catch remaining high-entropy runs after keywords.
    let re = Regex::new(r"\b[A-Za-z0-9+/_\-]{40,}={0,2}\b").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        let tok = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        if tok == REDACTED || tok.contains('[') {
            return tok.to_string();
        }
        if looks_high_entropy(tok) {
            REDACTED.to_string()
        } else {
            tok.to_string()
        }
    })
    .into_owned()
}

fn looks_high_entropy(s: &str) -> bool {
    if s.len() < 40 {
        return false;
    }
    let has_digit = s.chars().any(|c| c.is_ascii_digit());
    let has_lower = s.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
    let has_sym = s.chars().any(|c| matches!(c, '+' | '/' | '_' | '-' | '='));
    // Require mixed classes so normal prose/paths are not wiped.
    let classes = [has_digit, has_lower, has_upper, has_sym]
        .into_iter()
        .filter(|&x| x)
        .count();
    classes >= 3
}

/// Deep-redact a JSON value. Sensitive keys always redact string values.
pub fn redact_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(redact_text(s)),
        Value::Array(a) => Value::Array(a.iter().map(redact_value).collect()),
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                if is_sensitive_key(k) {
                    match val {
                        Value::String(_) | Value::Number(_) | Value::Bool(_) => {
                            out.insert(k.clone(), Value::String(REDACTED.into()));
                        }
                        Value::Null => {
                            out.insert(k.clone(), Value::Null);
                        }
                        other => {
                            out.insert(k.clone(), redact_value(other));
                        }
                    }
                } else {
                    out.insert(k.clone(), redact_value(val));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Redact then bound payload size for diagnostics.
/// Oversized payloads become `{truncated, originalBytes, preview, omittedBytes}`.
pub fn bound_payload(v: &Value, max_bytes: usize) -> Value {
    let redacted = redact_value(v);
    match serde_json::to_vec(&redacted) {
        Ok(bytes) if bytes.len() <= max_bytes => redacted,
        Ok(bytes) => {
            let take = max_bytes.min(bytes.len()).min(4096);
            let preview_raw = String::from_utf8_lossy(&bytes[..take]).into_owned();
            let preview = redact_text(&preview_raw);
            let meta = TruncationMeta {
                truncated: true,
                original_bytes: bytes.len(),
                preview,
                omitted_bytes: Some(bytes.len().saturating_sub(take)),
            };
            serde_json::to_value(meta).unwrap_or_else(|_| {
                serde_json::json!({
                    "truncated": true,
                    "originalBytes": bytes.len(),
                    "preview": REDACTED,
                })
            })
        }
        Err(_) => serde_json::json!({ "unserializable": true }),
    }
}

/// Convenience: redact + bound with default diagnostic budget.
pub fn prepare_diagnostic(v: &Value) -> Value {
    bound_payload(v, DEFAULT_DIAGNOSTIC_MAX_BYTES)
}

/// Redact a text buffer for logs (no JSON structure).
pub fn redact_log_line(line: &str) -> String {
    let redacted = redact_text(line);
    const MAX: usize = 4 * 1024;
    if redacted.len() <= MAX {
        redacted
    } else {
        format!(
            "{}… [truncated originalBytes={}]",
            &redacted[..MAX],
            redacted.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer_and_api_key() {
        let s = "Authorization: Bearer secret-token-xyz api_key=abc123";
        let out = redact_text(s);
        assert!(!out.contains("secret-token-xyz"), "{out}");
        assert!(!out.contains("abc123"), "{out}");
        assert!(out.contains(REDACTED), "{out}");
    }

    #[test]
    fn redacts_json_keys() {
        let v = serde_json::json!({
            "access_token": "tok123",
            "apiKey": "key-999",
            "password": "hunter2",
            "ok": "yes"
        });
        let r = redact_value(&v);
        assert_eq!(r["access_token"], REDACTED);
        assert_eq!(r["apiKey"], REDACTED);
        assert_eq!(r["password"], REDACTED);
        assert_eq!(r["ok"], "yes");
    }

    #[test]
    fn redacts_url_credentials() {
        let s = "fetch https://user:p@ssw0rd@example.com/v1/data";
        let out = redact_text(s);
        assert!(!out.contains("p@ssw0rd"), "{out}");
        assert!(out.contains("example.com"), "{out}");
        assert!(out.contains(REDACTED), "{out}");
    }

    #[test]
    fn redacts_query_token_params() {
        let s = "https://api.example.com/x?access_token=supersecretvalue&q=1";
        let out = redact_text(s);
        assert!(!out.contains("supersecretvalue"), "{out}");
        assert!(out.contains("access_token"), "{out}");
        assert!(out.contains(REDACTED), "{out}");
    }

    #[test]
    fn redacts_env_style_secrets() {
        let s = "export OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz0123456789";
        let out = redact_text(s);
        assert!(
            !out.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"),
            "{out}"
        );
        assert!(out.contains(REDACTED), "{out}");
    }

    #[test]
    fn bound_payload_writes_truncation_meta() {
        let big = "x".repeat(200);
        let v = serde_json::json!({ "blob": big });
        let out = bound_payload(&v, 80);
        assert_eq!(out["truncated"], true);
        assert!(out["originalBytes"].as_u64().unwrap() > 80);
        assert!(out.get("preview").is_some());
        assert!(out.get("omittedBytes").is_some());
    }

    #[test]
    fn does_not_redact_ordinary_paths() {
        let s = "reading src/server.ts and writing src/store.ts";
        let out = redact_text(s);
        assert_eq!(out, s);
    }
}
