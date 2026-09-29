//! ACP permission gate + transport wrapper (phase 02).
//!
//! Two harvest mechanisms:
//!
//! - Permission gate: clean-room port of the oh-my-pi mechanism (MIT) behind
//!   citation c010 (`7918ebae503236a7f8db6b77557dd25552c101cf133278416d394b8cf7adf38c`).
//!   No oh-my-pi code is copied: the re-derived rules are — a FIXED
//!   required-tool map (every tool names its gate class up front; unknown
//!   tools fail closed, never default-allow), exactly 4 permission options
//!   (`allow-once` / `allow-session` / `require-approval` / `deny`; unknown
//!   option IDs fail closed), and destructive edit-intent inspection (a
//!   write-class call touching secret-adjacent paths or escaping the
//!   workspace always escalates to `require-approval`, regardless of the
//!   granted option).
//! - Transport: dependency on the published `acp-http-adapter` crate
//!   (rivet-dev/sandbox-agent, Apache-2.0) behind citation c011
//!   (`bf562d6616249a03020dd1293c41f1bb5b0acaf233372c589200959e9be4e9bc`),
//!   pinned EXACT `=0.4.2` (0.x churn — no caret/range uptake; see
//!   `studio-core/Cargo.toml` for the lockfile-impact note). The crate is
//!   wrapped behind the [`AcpTransport`] trait: every bridged call passes
//!   [`decide`] FIRST (permission-callback verification lives in OUR gate,
//!   not in the dependency), and the crate's `run_server` (socket bind) is
//!   never constructed or called anywhere in this module or its tests —
//!   tests use [`FakeTransport`] only (V1 no-socket guard).

use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

/// Typed ACP failures. No `String` errors cross this boundary.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcpGateError {
    #[error("unknown ACP permission option {found}: fail-closed to deny")]
    UnknownOption { found: String },
    #[error("unknown ACP tool {tool}: no gate class — fail-closed to deny")]
    UnknownTool { tool: String },
    #[error("ACP denied: {reason}")]
    Denied { reason: String },
    #[error("ACP requires approval: {reason}")]
    RequiresApproval { reason: String },
    #[error("ACP transport: {0}")]
    Transport(String),
}

/// The four permission options. Wire IDs are exact; anything else is
/// [`AcpGateError::UnknownOption`] (fail-closed, never default-allow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionOption {
    AllowOnce,
    AllowSession,
    RequireApproval,
    Deny,
}

impl PermissionOption {
    pub fn parse(id: &str) -> Result<Self, AcpGateError> {
        match id {
            "allow-once" => Ok(PermissionOption::AllowOnce),
            "allow-session" => Ok(PermissionOption::AllowSession),
            "require-approval" => Ok(PermissionOption::RequireApproval),
            "deny" => Ok(PermissionOption::Deny),
            other => Err(AcpGateError::UnknownOption {
                found: other.into(),
            }),
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            PermissionOption::AllowOnce => "allow-once",
            PermissionOption::AllowSession => "allow-session",
            PermissionOption::RequireApproval => "require-approval",
            PermissionOption::Deny => "deny",
        }
    }
}

/// Fixed gate class per tool. The map is CLOSED: tools not listed here are
/// [`AcpGateError::UnknownTool`], never implicitly read-class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    Read,
    Write,
    Execute,
}

pub fn tool_class(tool: &str) -> Result<ToolClass, AcpGateError> {
    match tool {
        "read" | "code_search" | "tool_open" | "kv_get" | "web_search" | "retrieve_docs"
        | "syntax_check" => Ok(ToolClass::Read),
        "write" | "edit" | "patch" | "plan_open" | "dag_commit" => Ok(ToolClass::Write),
        "runProcess" | "sast" | "sbom" => Ok(ToolClass::Execute),
        other => Err(AcpGateError::UnknownTool { tool: other.into() }),
    }
}

/// Destructive edit-intent inspection: a write-class call is destructive when
/// it touches secret-adjacent paths or escapes the workspace (`..` or an
/// absolute path). Destructive intent ALWAYS escalates to `require-approval`.
pub fn is_destructive_intent(tool: &str, target: Option<&str>) -> bool {
    if !matches!(
        tool_class(tool),
        Ok(ToolClass::Write) | Ok(ToolClass::Execute)
    ) {
        return false;
    }
    let Some(t) = target else { return false };
    let low = t.to_lowercase();
    if t.contains("..") || t.starts_with('/') {
        return true;
    }
    [
        "auth", "secret", "crypto", "ledger", "migrate", "vault", "keyring",
    ]
    .iter()
    .any(|s| low.contains(s))
}

/// Gate verdict. `Allow` carries the option that granted it (audit trail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow { via: PermissionOption },
    RequireApproval { reason: String },
    Deny { reason: String },
}

/// Decide a tool call. Fail-closed order: unknown option → unknown tool →
/// explicit deny → destructive intent → class rules. Every refusal is typed.
pub fn decide(tool: &str, option_id: &str, target: Option<&str>) -> Result<Decision, AcpGateError> {
    let option = PermissionOption::parse(option_id)?;
    let class = tool_class(tool)?;
    if option == PermissionOption::Deny {
        return Ok(Decision::Deny {
            reason: format!("{tool} explicitly denied"),
        });
    }
    if is_destructive_intent(tool, target) {
        return Ok(Decision::RequireApproval {
            reason: format!(
                "destructive edit intent: {tool} on {}",
                target.unwrap_or("?")
            ),
        });
    }
    match (class, option) {
        (ToolClass::Read, PermissionOption::AllowOnce)
        | (ToolClass::Read, PermissionOption::AllowSession) => Ok(Decision::Allow { via: option }),
        (ToolClass::Read, PermissionOption::RequireApproval) => Ok(Decision::RequireApproval {
            reason: format!("{tool} gated by require-approval"),
        }),
        // Mutation needs a session-scoped grant; a one-shot allow is not
        // enough — it escalates to the approval service (fail-CLOSED: more
        // restrictive, never a silent allow).
        (ToolClass::Write, PermissionOption::AllowSession) => Ok(Decision::Allow { via: option }),
        (ToolClass::Write, _) => Ok(Decision::RequireApproval {
            reason: format!("{tool} mutates: approval + fencing lease required"),
        }),
        // Execution never rides a bare allow: approval or deny only.
        (ToolClass::Execute, PermissionOption::RequireApproval) => Ok(Decision::RequireApproval {
            reason: format!("{tool} executes: approval required"),
        }),
        (ToolClass::Execute, _) => Ok(Decision::RequireApproval {
            reason: format!("{tool} executes: bare allow insufficient, approval required"),
        }),
        (_, PermissionOption::Deny) => Ok(Decision::Deny {
            reason: format!("{tool} explicitly denied"),
        }),
    }
}

/// Minimal ACP transport surface: JSON-RPC bridge calls. The real impl wraps
/// `acp_http_adapter`; the fake covers the permission-callback contract in
/// tests without sockets or subprocesses.
///
/// The future is boxed (`Pin<Box<dyn ...>>`) so the trait stays object-safe:
/// gate code holds `&dyn AcpTransport` and the V1 replay harness swaps fakes
/// without generics.
pub trait AcpTransport: Send + Sync {
    /// Bridge one JSON-RPC payload. Implementations MUST be called only after
    /// [`decide`] allows — see [`post_gated`].
    fn post(
        &self,
        payload: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, AcpGateError>> + Send + '_>>;
}

/// Production transport: delegates to an already-running
/// `acp_http_adapter` runtime. Construction takes the runtime — this module
/// never spawns subprocesses and never calls `run_server` (socket bind).
pub struct RealAcpTransport {
    runtime: Arc<acp_http_adapter::process::AdapterRuntime>,
}

impl RealAcpTransport {
    pub fn new(runtime: Arc<acp_http_adapter::process::AdapterRuntime>) -> Self {
        Self { runtime }
    }
}

impl AcpTransport for RealAcpTransport {
    fn post(
        &self,
        payload: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, AcpGateError>> + Send + '_>>
    {
        Box::pin(async move {
            match self
                .runtime
                .post(payload)
                .await
                .map_err(|e| AcpGateError::Transport(e.to_string()))?
            {
                acp_http_adapter::process::PostOutcome::Response(v) => Ok(v),
                // Fire-and-forget accepted: no response body to bridge.
                acp_http_adapter::process::PostOutcome::Accepted => Ok(Value::Null),
            }
        })
    }
}

/// Test transport: scripted responses + a record of every call that passed
/// the gate. Never touches the network (V1 no-socket guard).
#[derive(Debug, Default)]
pub struct FakeTransport {
    calls: std::sync::Mutex<Vec<Value>>,
    response: std::sync::Mutex<Value>,
}

impl FakeTransport {
    pub fn new(response: Value) -> Self {
        Self {
            calls: std::sync::Mutex::new(vec![]),
            response: std::sync::Mutex::new(response),
        }
    }

    pub fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
}

impl AcpTransport for FakeTransport {
    fn post(
        &self,
        payload: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, AcpGateError>> + Send + '_>>
    {
        Box::pin(async move {
            self.calls.lock().unwrap().push(payload);
            Ok(self.response.lock().unwrap().clone())
        })
    }
}

/// Gated bridge call: the permission gate runs FIRST; the transport is
/// touched only on `Allow`. `RequireApproval`/`Deny` are typed refusals and
/// the payload never leaves the process.
pub async fn post_gated(
    transport: &dyn AcpTransport,
    tool: &str,
    option_id: &str,
    target: Option<&str>,
    payload: Value,
) -> Result<Value, AcpGateError> {
    match decide(tool, option_id, target)? {
        Decision::Allow { .. } => transport.post(payload).await,
        Decision::RequireApproval { reason } => Err(AcpGateError::RequiresApproval { reason }),
        Decision::Deny { reason } => Err(AcpGateError::Denied { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_options_and_tools_fail_closed() {
        assert!(matches!(
            PermissionOption::parse("allow-sometimes").unwrap_err(),
            AcpGateError::UnknownOption { .. }
        ));
        assert!(matches!(
            decide("ghost-tool", "allow-session", None).unwrap_err(),
            AcpGateError::UnknownTool { .. }
        ));
        // Unknown option beats even a known tool: parse order is fail-closed.
        assert!(matches!(
            decide("read", "allow-sometimes", None).unwrap_err(),
            AcpGateError::UnknownOption { .. }
        ));
    }

    #[test]
    fn read_allows_with_grant_denies_with_deny() {
        assert!(matches!(
            decide("read", "allow-once", None).unwrap(),
            Decision::Allow { .. }
        ));
        assert!(matches!(
            decide("code_search", "allow-session", None).unwrap(),
            Decision::Allow { .. }
        ));
        assert!(matches!(
            decide("read", "deny", None).unwrap(),
            Decision::Deny { .. }
        ));
        assert!(matches!(
            decide("read", "require-approval", None).unwrap(),
            Decision::RequireApproval { .. }
        ));
    }

    #[test]
    fn write_needs_session_grant_and_approval_service() {
        assert!(matches!(
            decide("write", "allow-session", Some("src/app.rs")).unwrap(),
            Decision::Allow { .. }
        ));
        // One-shot allow on mutation escalates to approval (fail-closed).
        assert!(matches!(
            decide("write", "allow-once", Some("src/app.rs")).unwrap(),
            Decision::RequireApproval { .. }
        ));
        // Execution never rides a bare allow.
        assert!(matches!(
            decide("runProcess", "allow-session", None).unwrap(),
            Decision::RequireApproval { .. }
        ));
        assert!(matches!(
            decide("runProcess", "deny", None).unwrap(),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn destructive_intent_always_escalates() {
        // Secret-adjacent target: even a session grant is insufficient.
        let d = decide("write", "allow-session", Some("src/auth/keys.rs")).unwrap();
        assert!(matches!(d, Decision::RequireApproval { .. }));
        // Workspace escape: same escalation.
        let d = decide("edit", "allow-session", Some("../outside.rs")).unwrap();
        assert!(matches!(d, Decision::RequireApproval { .. }));
        let d = decide("patch", "allow-session", Some("/etc/passwd")).unwrap();
        assert!(matches!(d, Decision::RequireApproval { .. }));
        // Read-class tools never trigger intent inspection.
        assert!(matches!(
            decide("read", "allow-session", Some("src/auth/keys.rs")).unwrap(),
            Decision::Allow { .. }
        ));
        // Explicit deny still wins over intent escalation.
        assert!(matches!(
            decide("write", "deny", Some("src/auth/keys.rs")).unwrap(),
            Decision::Deny { .. }
        ));
    }

    #[tokio::test]
    async fn gated_post_checks_permission_before_transport() {
        let fake = FakeTransport::new(json!({"ok": true}));
        // Allowed read bridges.
        let out = post_gated(&fake, "read", "allow-once", None, json!({"m": 1}))
            .await
            .unwrap();
        assert_eq!(out, json!({"ok": true}));
        assert_eq!(fake.calls().len(), 1);
        // Denied exec never reaches the transport.
        let err = post_gated(&fake, "runProcess", "deny", None, json!({"m": 2}))
            .await
            .unwrap_err();
        assert!(matches!(err, AcpGateError::Denied { .. }));
        // Approval-gated write never reaches the transport either.
        let err = post_gated(&fake, "write", "allow-once", Some("a.rs"), json!({"m": 3}))
            .await
            .unwrap_err();
        assert!(matches!(err, AcpGateError::RequiresApproval { .. }));
        assert_eq!(fake.calls().len(), 1);
        // Unknown tool: typed, transport untouched.
        assert!(matches!(
            post_gated(&fake, "ghost", "allow-session", None, json!({}))
                .await
                .unwrap_err(),
            AcpGateError::UnknownTool { .. }
        ));
        assert_eq!(fake.calls().len(), 1);
    }
}
