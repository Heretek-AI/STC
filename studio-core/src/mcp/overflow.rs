//! Overflow KV: wire payloads >10KB collapse to `mcp:overflow:<agentId>` pointers.
//! Same pointer both channels; `kv_get` exempt. TTL 24h (enforced on read).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WIRE_LIMIT_BYTES: usize = 10 * 1024;
pub const OVERFLOW_TTL_SECS: u64 = 24 * 3600;

#[derive(Debug, Clone)]
struct Entry {
    bytes: Vec<u8>,
    inserted_secs: u64,
}

#[derive(Debug, Default)]
pub struct OverflowKv {
    inner: Mutex<HashMap<String, Entry>>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl OverflowKv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn key_for(agent: &str) -> String {
        format!("mcp:overflow:{agent}")
    }

    /// Store payload, return pointer key. `kv_get` path uses `fetch` (exempt from re-collapse).
    pub fn store(&self, agent: &str, payload: Vec<u8>) -> String {
        let key = Self::key_for(agent);
        self.inner.lock().unwrap().insert(
            key.clone(),
            Entry {
                bytes: payload,
                inserted_secs: now_secs(),
            },
        );
        key
    }

    pub fn fetch(&self, key: &str) -> Option<Vec<u8>> {
        let mut g = self.inner.lock().unwrap();
        let e = g.get(key)?.clone();
        if now_secs().saturating_sub(e.inserted_secs) > OVERFLOW_TTL_SECS {
            g.remove(key);
            return None;
        }
        Some(e.bytes)
    }

    /// Collapse wire payloads over the limit to a pointer. Under-limit passes through.
    pub fn collapse(&self, agent: &str, payload: Vec<u8>) -> Collapse {
        if payload.len() > WIRE_LIMIT_BYTES {
            let key = self.store(agent, payload);
            Collapse::Pointer(key)
        } else {
            Collapse::Inline(payload)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Collapse {
    Inline(Vec<u8>),
    Pointer(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_over_limit() {
        let kv = OverflowKv::new();
        let big = vec![0u8; WIRE_LIMIT_BYTES + 1];
        match kv.collapse("a1", big) {
            Collapse::Pointer(k) => {
                assert_eq!(k, "mcp:overflow:a1");
                assert!(kv.fetch(&k).is_some());
            }
            _ => panic!("must collapse"),
        }
        let small = vec![0u8; 100];
        assert!(matches!(kv.collapse("a1", small), Collapse::Inline(_)));
    }

    #[test]
    fn one_mb_result_becomes_pointer() {
        let kv = OverflowKv::new();
        let mb = vec![1u8; 1024 * 1024];
        match kv.collapse("coder-1", mb) {
            Collapse::Pointer(k) => assert!(k.starts_with("mcp:overflow:")),
            _ => panic!("1MB must not flood the wire"),
        }
    }
}
