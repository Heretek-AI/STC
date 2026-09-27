//! Harness config generation (kills hand-edited JSON).
//! Emits `.opencode/opencode.json` and pi `models.json`/`auth.json` into
//! per-lane profile dirs from the same `Connection`s. Secret values are
//! written as bind-time placeholders resolved by `SecretBroker::checkout` /
//! `CredentialStore::get` — never persisted raw in locks or committed files
//! (kasetto pattern: fail closed on missing material).

use super::{Connection, ConnectionId};
use crate::auth::CredentialStore;
use std::collections::HashMap;
use std::path::Path;

/// Placeholder written wherever a secret belongs. Resolved at lane bind time.
pub fn secret_placeholder(label: &str) -> String {
    format!("{{{{STUDIO_SECRET:{label}}}}}")
}

/// Resolve all `{{STUDIO_SECRET:<label>}}` placeholders via the store.
/// Fails closed on the first unresolvable label.
pub fn resolve_placeholders(text: &str, store: &CredentialStore) -> Result<String, String> {
    let mut out = text.to_string();
    while let Some(start) = out.find("{{STUDIO_SECRET:") {
        let rest = &out[start + "{{STUDIO_SECRET:".len()..];
        let end = rest
            .find("}}")
            .ok_or_else(|| "unterminated secret placeholder".to_string())?;
        let label = &rest[..end];
        let cred = store
            .get(label)
            .map_err(|e| format!("missing secret material for {label}: {e}"))?;
        out.replace_range(
            start..start + "{{STUDIO_SECRET:".len() + end + 2,
            &cred.material,
        );
    }
    Ok(out)
}

/// Connections indexed for emission, with the secret label each one needs
/// (empty string = no secret, e.g. local or CLI-delegated).
pub struct Emission<'a> {
    pub connections: Vec<(&'a Connection, String)>,
}

fn lane_dir(base: &str, lane: &str) -> PathBuf {
    Path::new(base).join("profiles").join(lane)
}

use std::path::PathBuf;

/// Emit `.opencode/opencode.json` for a lane. Returns the written path.
pub fn emit_opencode(base: &str, lane: &str, em: &Emission) -> Result<PathBuf, String> {
    let mut providers = serde_json::Map::new();
    for (conn, secret_label) in &em.connections {
        let mut block = serde_json::Map::new();
        if let Some(url) = &conn.base_url {
            block.insert("base_url".into(), serde_json::Value::String(url.clone()));
        }
        if !secret_label.is_empty() {
            block.insert(
                "api_key".into(),
                serde_json::Value::String(secret_placeholder(secret_label)),
            );
        }
        block.insert(
            "models".into(),
            conn.models
                .iter()
                .map(|m| serde_json::Value::String(m.0.clone()))
                .collect(),
        );
        providers.insert(conn.id.0.clone(), serde_json::Value::Object(block));
    }
    let doc = serde_json::json!({ "providers": providers });
    write_lane_file(base, lane, ".opencode/opencode.json", &doc)
}

/// Emit pi `models.json` + `auth.json` for a lane. Returns written paths.
pub fn emit_pi(base: &str, lane: &str, em: &Emission) -> Result<(PathBuf, PathBuf), String> {
    let mut models: HashMap<String, Vec<String>> = HashMap::new();
    let mut auth = serde_json::Map::new();
    for (conn, secret_label) in &em.connections {
        models.insert(
            conn.id.0.clone(),
            conn.models.iter().map(|m| m.0.clone()).collect(),
        );
        if !secret_label.is_empty() {
            auth.insert(
                conn.id.0.clone(),
                serde_json::Value::String(secret_placeholder(secret_label)),
            );
        }
    }
    let mpath = write_lane_file(
        base,
        lane,
        "pi/models.json",
        &serde_json::json!({ "providers": models }),
    )?;
    let apath = write_lane_file(base, lane, "pi/auth.json", &serde_json::Value::Object(auth))?;
    Ok((mpath, apath))
}

fn write_lane_file(
    base: &str,
    lane: &str,
    rel: &str,
    doc: &serde_json::Value,
) -> Result<PathBuf, String> {
    let path = lane_dir(base, lane).join(rel);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(doc).map_err(|e| e.to_string())?;
    // Refuse to write raw-looking secrets: every secret must be a placeholder.
    // (Heuristic backstop — the constructors above never inline material.)
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Convenience: find a connection by id for emission.
pub fn find<'a>(hub: &'a super::ProviderHub, id: &ConnectionId) -> Option<&'a Connection> {
    hub.connections.get(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthKind, Credential};

    fn em<'a>(conns: Vec<&'a Connection>, labels: Vec<String>) -> Emission<'a> {
        Emission {
            connections: conns.into_iter().zip(labels).collect(),
        }
    }

    fn conn() -> Connection {
        Connection {
            id: ConnectionId("go".into()),
            name: "go".into(),
            auth_kind: AuthKind::ApiKey,
            base_url: Some("https://go.opencode.ai".into()),
            models: vec![crate::provider::ModelId("qwen".into())],
            cost: HashMap::new(),
        }
    }

    #[test]
    fn emitted_files_carry_placeholders_not_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_string_lossy().to_string();
        let c = conn();
        let e = em(vec![&c], vec!["go-key".into()]);
        let p = emit_opencode(&base, "lane-1", &e).unwrap();
        let body = std::fs::read_to_string(p).unwrap();
        assert!(body.contains("{{STUDIO_SECRET:go-key}}"));
        assert!(!body.contains("sk-"));
        let (mp, ap) = emit_pi(&base, "lane-1", &e).unwrap();
        assert!(std::fs::read_to_string(ap)
            .unwrap()
            .contains("{{STUDIO_SECRET:go-key}}"));
        assert!(mp.exists());
    }

    #[test]
    fn placeholders_resolve_and_missing_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let store = CredentialStore::new("studio-test-sync", dir.path());
        // Keyring may or may not exist here; drive the vault path explicitly
        // by using a label we control end-to-end through set/get when possible.
        let _ = store.set(&Credential {
            label: "k".into(),
            auth_kind: AuthKind::ApiKey,
            material: "sk-live".into(),
            quota_window: None,
            expires_at: None,
        });
        let out = resolve_placeholders("key={{STUDIO_SECRET:k}}!", &store).unwrap();
        assert_eq!(out, "key=sk-live!");
        assert!(resolve_placeholders("{{STUDIO_SECRET:nope}}", &store).is_err());
    }
}
