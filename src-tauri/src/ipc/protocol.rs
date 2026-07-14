//! NDJSON IPC message types (hello, request/response, events).

use crate::fingerprint::BinaryFingerprint;
use crate::version::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client roles that may connect to the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientRole {
    Mcp,
    Cli,
    GuiHost,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub r#type: String,
    pub request_id: String,
    pub protocol_version: u32,
    pub role: ClientRole,
    pub client_version: String,
    pub binary_path: String,
    pub binary_fingerprint: BinaryFingerprint,
    pub pid: u32,
}

impl Hello {
    pub fn new(
        request_id: impl Into<String>,
        role: ClientRole,
        client_version: impl Into<String>,
        binary_path: impl Into<String>,
        fingerprint: BinaryFingerprint,
        pid: u32,
    ) -> Self {
        Self {
            r#type: "hello".into(),
            request_id: request_id.into(),
            protocol_version: PROTOCOL_VERSION,
            role,
            client_version: client_version.into(),
            binary_path: binary_path.into(),
            binary_fingerprint: fingerprint,
            pid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelloStatus {
    Ok,
    Restarting,
    ReplacementDeferred,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HelloAck {
    pub r#type: String,
    pub request_id: String,
    pub protocol_version: u32,
    pub daemon_version: String,
    pub status: HelloStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_instance_id: Option<String>,
}

impl HelloAck {
    pub fn ok(
        request_id: impl Into<String>,
        daemon_version: impl Into<String>,
        instance: impl Into<String>,
    ) -> Self {
        Self {
            r#type: "hello_ack".into(),
            request_id: request_id.into(),
            protocol_version: PROTOCOL_VERSION,
            daemon_version: daemon_version.into(),
            status: HelloStatus::Ok,
            reason: None,
            retry_until: None,
            daemon_instance_id: Some(instance.into()),
        }
    }

    pub fn incompatible(request_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            r#type: "hello_ack".into(),
            request_id: request_id.into(),
            protocol_version: PROTOCOL_VERSION,
            daemon_version: crate::version::APP_VERSION.into(),
            status: HelloStatus::Incompatible,
            reason: Some(reason.into()),
            retry_until: None,
            daemon_instance_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub r#type: String,
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn new(request_id: impl Into<String>, method: impl Into<String>, params: Value) -> Self {
        Self {
            r#type: "request".into(),
            request_id: request_id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub r#type: String,
    pub request_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(request_id: impl Into<String>, result: Value) -> Self {
        Self {
            r#type: "response".into(),
            request_id: request_id.into(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(
        request_id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            r#type: "response".into(),
            request_id: request_id.into(),
            ok: false,
            result: None,
            error: Some(RpcError {
                code: code.into(),
                message: message.into(),
                retryable,
            }),
        }
    }
}

/// Generic event envelope used for live mutations / snapshot frames.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub r#type: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(flatten)]
    pub extra: MapFlatten,
}

/// Flatten helper so we can attach arbitrary payload fields without losing unknowns.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapFlatten {
    pub fields: serde_json::Map<String, Value>,
}

impl Serialize for MapFlatten {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.fields.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MapFlatten {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = serde_json::Map::deserialize(deserializer)?;
        Ok(Self { fields })
    }
}

/// GUI-host single-instance navigation commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method")]
pub enum GuiNavCommand {
    #[serde(rename = "gui.open_popover")]
    OpenPopover,
    #[serde(rename = "gui.open_task")]
    OpenTask {
        #[serde(rename = "taskId")]
        task_id: String,
    },
    #[serde(rename = "gui.open_history")]
    OpenHistory,
    #[serde(rename = "gui.open_settings")]
    OpenSettings {
        /// Optional settings section (e.g. `integrations` for `GrokTask setup`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    #[serde(rename = "gui.focus")]
    Focus,
    #[serde(rename = "gui.quit")]
    Quit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_roundtrip() {
        let h = Hello::new(
            "r1",
            ClientRole::Cli,
            "0.1.0",
            "/tmp/GrokTask",
            BinaryFingerprint {
                size: 1,
                mtime_ns: 2,
            },
            42,
        );
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["type"], "hello");
        assert_eq!(v["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["binaryFingerprint"]["mtimeNs"], 2);
        let back: Hello = serde_json::from_value(v).unwrap();
        assert_eq!(back.pid, 42);
    }

    #[test]
    fn response_error_shape() {
        let r = Response::err("1", "invalid_argument", "bad", false);
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"ok\":false"));
        assert!(s.contains("invalid_argument"));
    }
}
