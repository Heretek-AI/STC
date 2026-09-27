//! ACP v1 model-protocol bridge over NDJSON stdio (`adapters/acp`).
//! One JSON-RPC object per line on stdin/stdout; each bridge binds a session to
//! `task_id` + role manifest (`AcpSession`). `session/request_permission` answers
//! allow/deny from the manifest (double enforcement with the MCP gateway).

use studio_core::mcp::AgentRole;
use studio_core::session::acp::{AcpFrame, AcpSession};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn parse_role(s: &str) -> AgentRole {
    match s {
        "manager" => AgentRole::Manager,
        "researcher" => AgentRole::Researcher,
        "reviewer" => AgentRole::Reviewer,
        _ => AgentRole::Coder,
    }
}

#[tokio::main]
async fn main() {
    let task_id = std::env::args().nth(1).unwrap_or_else(|| "task-1".into());
    let role = std::env::args()
        .nth(2)
        .map(|r| parse_role(&r))
        .unwrap_or(AgentRole::Coder);
    let mut session = AcpSession::new(&task_id, role);
    eprintln!(
        "studio-acp: task={task_id} role={} (NDJSON stdio)",
        role.as_str()
    );

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let frame = match AcpFrame::decode(&line) {
            Ok(f) => f,
            Err(e) => {
                let err = AcpFrame {
                    jsonrpc: "2.0".into(),
                    id: None,
                    method: None,
                    params: None,
                    result: None,
                    error: Some(studio_core::session::acp::AcpError {
                        code: -32700,
                        message: e,
                    }),
                };
                let out = err.encode().unwrap_or_default();
                stdout.write_all(out.as_bytes()).await.ok();
                stdout.flush().await.ok();
                continue;
            }
        };
        let method = frame.method.as_deref().unwrap_or("");
        let result_body = match method {
            "initialize" => {
                Some(serde_json::json!({"protocolVersion": "v1", "taskId": session.task_id}))
            }
            "session/new" => {
                Some(serde_json::json!({"taskId": session.task_id, "role": session.role.as_str()}))
            }
            "session/prompt" => {
                let text = frame
                    .params
                    .as_ref()
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let _queued = session.prompt(text);
                Some(serde_json::json!({"queued": true, "taskId": session.task_id}))
            }
            "session/request_permission" => {
                let tool = frame
                    .params
                    .as_ref()
                    .and_then(|p| p.get("tool"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let (_, resp) = session.request_permission(tool);
                resp.result
            }
            _ => None,
        };
        let response = match result_body {
            Some(body) => AcpFrame {
                jsonrpc: "2.0".into(),
                id: frame.id.clone(),
                method: None,
                params: None,
                result: Some(body),
                error: None,
            },
            None => AcpFrame {
                jsonrpc: "2.0".into(),
                id: frame.id.clone(),
                method: None,
                params: None,
                result: None,
                error: Some(studio_core::session::acp::AcpError {
                    code: -32601,
                    message: format!("unknown method {method}"),
                }),
            },
        };
        if let Ok(out) = response.encode() {
            stdout.write_all(out.as_bytes()).await.ok();
            stdout.flush().await.ok();
        }
    }
}
