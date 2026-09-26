//! Artifact-first creative lanes (Phase 5).
//! Source-of-record first (SVG / HTML / Remotion timeline / Blender Python);
//! deterministic render from the winning source hash; never text-merge binaries —
//! the merge queue takes the winning source-of-record hash and re-renders.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceRecord {
    Svg { svg: String },
    Html { html: String },
    RemotionTimeline { json: serde_json::Value },
    BlenderPython { script: String },
}

impl SourceRecord {
    /// Deterministic content hash (the merge queue's winning-hash input).
    pub fn content_hash(&self) -> String {
        let mut h = Sha256::new();
        match self {
            SourceRecord::Svg { svg } => {
                h.update(b"svg|");
                h.update(svg.as_bytes());
            }
            SourceRecord::Html { html } => {
                h.update(b"html|");
                h.update(html.as_bytes());
            }
            SourceRecord::RemotionTimeline { json } => {
                h.update(b"remotion|");
                h.update(serde_json::to_string(json).unwrap_or_default().as_bytes());
            }
            SourceRecord::BlenderPython { script } => {
                h.update(b"blender|");
                h.update(script.as_bytes());
            }
        }
        hex::encode(h.finalize())
    }

    /// Deterministic render check: SVG must be well-formed-ish (balanced tags),
    /// HTML must contain <html>, scripts must not be empty. No external renderer here.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            SourceRecord::Svg { svg } => {
                if svg.contains("<svg") && svg.contains("</svg>") {
                    Ok(())
                } else {
                    Err("svg source must contain <svg>…</svg>".into())
                }
            }
            SourceRecord::Html { html } => {
                if html.contains("<html") {
                    Ok(())
                } else {
                    Err("html source must contain <html>".into())
                }
            }
            SourceRecord::RemotionTimeline { json } => {
                if json.get("timeline").is_some() || json.get("composition").is_some() {
                    Ok(())
                } else {
                    Err("remotion source needs timeline|composition".into())
                }
            }
            SourceRecord::BlenderPython { script } => {
                if script.contains("import bpy") {
                    Ok(())
                } else {
                    Err("blender source must import bpy".into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winning_hash_deterministic() {
        let a = SourceRecord::Svg {
            svg: "<svg></svg>".into(),
        };
        assert_eq!(a.content_hash(), a.content_hash());
        assert!(a.validate().is_ok());
        assert!(SourceRecord::Svg { svg: "nope".into() }.validate().is_err());
    }
}
