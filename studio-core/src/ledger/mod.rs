//! Hash-chain plan ledger: `.swarm/plan-ledger.jsonl` authoritative, `plan.json` projection.
//! Anchor (by symbol): opencode-swarm `src/plan/ledger.ts`
//! (`seq`, `plan_hash_before/after`, `source`, `payload_hash`, root `plan_created`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("io: {0}")]
    Io(String),
    #[error("chain broken at seq {seq}: expected {expected}, got {got}")]
    ChainBroken {
        seq: u64,
        expected: String,
        got: String,
    },
    #[error("root must be plan_created, found {0}")]
    BadRoot(String),
    #[error("json: {0}")]
    Json(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEvent {
    pub seq: u64,
    pub timestamp: String,
    pub plan_id: String,
    pub event_type: String,
    pub task_id: Option<String>,
    pub source: String,
    pub plan_hash_before: String,
    pub plan_hash_after: String,
    pub payload_hash: String,
}

pub fn hash_json(v: &serde_json::Value) -> String {
    let mut h = Sha256::new();
    h.update(serde_json::to_string(v).unwrap_or_default().as_bytes());
    hex::encode(h.finalize())
}

pub struct Ledger {
    path: std::path::PathBuf,
}

impl Ledger {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn append(
        &self,
        plan_id: &str,
        event_type: &str,
        task_id: Option<&str>,
        source: &str,
        plan_before: &serde_json::Value,
        plan_after: &serde_json::Value,
    ) -> Result<LedgerEvent, LedgerError> {
        let prev = self.read_all().unwrap_or_default();
        let seq = prev.len() as u64 + 1;
        let before_h = hash_json(plan_before);
        let after_h = hash_json(plan_after);
        // chain: payload_hash binds prev after-hash + current content
        let mut h = Sha256::new();
        let prev_after = prev
            .last()
            .map(|e: &LedgerEvent| e.plan_hash_after.clone())
            .unwrap_or_else(|| "genesis".into());
        h.update(format!("{prev_after}|{plan_id}|{event_type}|{after_h}").as_bytes());
        let payload_hash = hex::encode(h.finalize());
        // enforce before-hash continuity (except genesis)
        if let Some(last) = prev.last() {
            if last.plan_hash_after != before_h {
                // tamper guard: workers may not rewrite history; fail closed
                return Err(LedgerError::ChainBroken {
                    seq,
                    expected: last.plan_hash_after.clone(),
                    got: before_h,
                });
            }
        }
        if seq == 1 && event_type != "plan_created" {
            return Err(LedgerError::BadRoot(event_type.into()));
        }
        let ev = LedgerEvent {
            seq,
            timestamp: chrono_timestamp(),
            plan_id: plan_id.into(),
            event_type: event_type.into(),
            task_id: task_id.map(|s| s.into()),
            source: source.into(),
            plan_hash_before: before_h,
            plan_hash_after: after_h,
            payload_hash,
        };
        if let Some(p) = self.path.parent() {
            fs::create_dir_all(p).map_err(|e| LedgerError::Io(e.to_string()))?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| LedgerError::Io(e.to_string()))?;
        writeln!(
            f,
            "{}",
            serde_json::to_string(&ev).map_err(|e| LedgerError::Json(e.to_string()))?
        )
        .map_err(|e| LedgerError::Io(e.to_string()))?;
        Ok(ev)
    }

    pub fn read_all(&self) -> Result<Vec<LedgerEvent>, LedgerError> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let text = fs::read_to_string(&self.path).map_err(|e| LedgerError::Io(e.to_string()))?;
        let mut out = vec![];
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str(line).map_err(|e| LedgerError::Json(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn verify(&self) -> Result<(), LedgerError> {
        let evs = self.read_all()?;
        for (i, e) in evs.iter().enumerate() {
            if e.seq != i as u64 + 1 {
                return Err(LedgerError::ChainBroken {
                    seq: e.seq,
                    expected: format!("seq {}", i + 1),
                    got: format!("seq {}", e.seq),
                });
            }
            if i == 0 && e.event_type != "plan_created" {
                return Err(LedgerError::BadRoot(e.event_type.clone()));
            }
            if i > 0 && evs[i - 1].plan_hash_after != e.plan_hash_before {
                return Err(LedgerError::ChainBroken {
                    seq: e.seq,
                    expected: evs[i - 1].plan_hash_after.clone(),
                    got: e.plan_hash_before.clone(),
                });
            }
        }
        Ok(())
    }
}

fn chrono_timestamp() -> String {
    // no chrono dep: use epoch millis
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn appends_and_verifies_chain() {
        let dir = tempfile::tempdir().unwrap();
        let led = Ledger::new(&dir.path().join("plan-ledger.jsonl"));
        let p0 = json!({"tasks":[]});
        let p1 = json!({"tasks":["t1"]});
        led.append("p1", "plan_created", None, "manager", &p0, &p0)
            .unwrap();
        led.append("p1", "task_added", Some("t1"), "manager", &p0, &p1)
            .unwrap();
        led.verify().unwrap();
    }

    #[test]
    fn rejects_rewritten_history() {
        let dir = tempfile::tempdir().unwrap();
        let led = Ledger::new(&dir.path().join("plan-ledger.jsonl"));
        let p0 = json!({"tasks":[]});
        let p1 = json!({"tasks":["t1"]});
        led.append("p1", "plan_created", None, "manager", &p0, &p0)
            .unwrap();
        // forging a different before-state must fail
        assert!(led
            .append("p1", "task_added", Some("t1"), "worker", &p1, &p1)
            .is_err());
    }
}
