//! ACP v1 bridge: JSON-RPC 2.0 objects framed as NDJSON (one object per line).
//! Each session binds `task_id` + role manifest; `session/request_permission`
//! is the second enforcement point after the gateway deny (double enforcement).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpFrame {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: Option<String>,
    pub params: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<AcpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpError {
    pub code: i32,
    pub message: String,
}

impl AcpFrame {
    pub fn request(id: i64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id.into()),
            method: Some(method.into()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// Encode to a single NDJSON line (no embedded newlines).
    pub fn encode(&self) -> Result<String, String> {
        let mut s = serde_json::to_string(self).map_err(|e| e.to_string())?;
        s.push('\n');
        Ok(s)
    }

    /// Decode one NDJSON line; rejects multi-line input and non-2.0 versions.
    pub fn decode(line: &str) -> Result<Self, String> {
        let t = line.trim();
        if t.is_empty() {
            return Err("empty frame".into());
        }
        if t.contains('\n') {
            return Err("frame must be a single NDJSON line".into());
        }
        let f: AcpFrame = serde_json::from_str(t).map_err(|e| e.to_string())?;
        if f.jsonrpc != "2.0" {
            return Err(format!("unsupported jsonrpc {}", f.jsonrpc));
        }
        Ok(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
}

pub struct AcpSession {
    pub task_id: String,
    pub role: crate::mcp::AgentRole,
    pub manifest: crate::mcp::RoleManifest,
    next_id: i64,
}

impl AcpSession {
    pub fn new(task_id: &str, role: crate::mcp::AgentRole) -> Self {
        Self {
            task_id: task_id.into(),
            role,
            manifest: crate::mcp::default_manifest(role),
            next_id: 1,
        }
    }

    fn alloc_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn initialize(&mut self) -> AcpFrame {
        let id = self.alloc_id();
        AcpFrame::request(
            id,
            "initialize",
            serde_json::json!({"protocolVersion": "v1"}),
        )
    }

    pub fn new_session(&mut self) -> AcpFrame {
        let id = self.alloc_id();
        AcpFrame::request(
            id,
            "session/new",
            serde_json::json!({"taskId": self.task_id, "role": self.role.as_str()}),
        )
    }

    pub fn prompt(&mut self, text: &str) -> AcpFrame {
        let id = self.alloc_id();
        AcpFrame::request(
            id,
            "session/prompt",
            serde_json::json!({"taskId": self.task_id, "text": text}),
        )
    }

    /// `session/request_permission` handler: structured refusal for out-of-scope tools.
    /// Gateway deny is enforcement #1; this is enforcement #2 (never allow on manifest miss).
    pub fn request_permission(&self, tool: &str) -> (PermissionDecision, AcpFrame) {
        match self.manifest.check(tool) {
            Ok(_) => (
                PermissionDecision::Allow,
                AcpFrame {
                    jsonrpc: "2.0".into(),
                    id: None,
                    method: None,
                    params: None,
                    result: Some(serde_json::json!({"decision": "allow"})),
                    error: None,
                },
            ),
            Err(reason) => (
                PermissionDecision::Deny(reason.clone()),
                AcpFrame {
                    jsonrpc: "2.0".into(),
                    id: None,
                    method: None,
                    params: None,
                    result: Some(serde_json::json!({"decision": "deny", "reason": reason})),
                    error: None,
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_roundtrip() {
        let f = AcpFrame::request(1, "session/prompt", serde_json::json!({}));
        let line = f.encode().unwrap();
        assert_eq!(line.lines().count(), 1);
        assert_eq!(AcpFrame::decode(&line).unwrap(), f);
    }

    #[test]
    fn rejects_bad_version() {
        assert!(AcpFrame::decode(r#"{"jsonrpc":"1.0"}"#).is_err());
    }

    #[test]
    fn permission_double_enforcement() {
        let s = AcpSession::new("t1", crate::mcp::AgentRole::Coder);
        let (d, _) = s.request_permission("write");
        assert_eq!(d, PermissionDecision::Allow);
        let (d, frame) = s.request_permission("sbom");
        assert!(matches!(d, PermissionDecision::Deny(_)));
        let body = frame.result.unwrap().to_string();
        assert!(body.contains("deny"));
    }
}
