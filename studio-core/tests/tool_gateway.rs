//! Phase-03 acceptance: registry loads from manifest, Allow/OnDemand/Deny
//! slicing, lazy schema load with bounded index, unknown-tool typed denial,
//! write-detect + overflow, tier/gate pairing, and the V3 fixture farm.
//! QA-R1 pins: OnDemand + lens enforced at the seam, invalid lens typed,
//! bundle submit catalog-bound, read-escape write-detect, redactor verdict.

use serde_json::json;
use std::time::Duration;
use studio_core::mcp::{bundles::*, fixtures::*, lens::*, registry::*, tiers::*};

fn manifest() -> String {
    json!([
        {"name": "read", "description": "read a file", "version": "1.2.0",
         "tags": ["fs"], "schema": {"type": "object",
          "properties": {"path": {"type": "string", "description": "file to read"}},
          "required": ["path"]}},
        {"name": "write", "description": "write a file", "version": "1.2.0",
         "tags": ["fs"], "schema": {"type": "object",
          "properties": {"path": {"type": "string", "description": "file to write"}}}},
        {"name": "runProcess", "description": "run a process", "version": "2.0.0",
         "tags": ["exec"], "schema": {"type": "object", "properties": {}}}
    ])
    .to_string()
}

fn catalog() -> Vec<String> {
    vec!["read".into(), "write".into(), "runProcess".into()]
}

struct OkExec;
impl ToolExecutor for OkExec {
    fn execute(&self, tool: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(json!({"tool": tool}))
    }
}

#[test]
fn registry_loads_from_manifest() {
    let reg = Registry::load_manifest(&manifest()).unwrap();
    assert_eq!(reg.len(), 3);
    assert!(reg.contains("read"));
}

#[test]
fn slicing_allow_ondemand_deny_holds() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let rows = reg
        .slice(&[
            ("read".into(), SliceMode::Allow),
            ("write".into(), SliceMode::OnDemand),
            ("runProcess".into(), SliceMode::Deny),
        ])
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].on_demand);
    assert!(rows[1].on_demand);
}

#[test]
fn lazy_schema_load_keeps_index_bounded() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let before = reg.index_bytes();
    assert!(before <= MAX_INDEX_BYTES);
    let schema = reg.tool_open("read").unwrap();
    assert!(schema.get("properties").is_some());
    assert!(reg.is_opened("read"));
    assert!(!reg.is_opened("write"));
    assert_eq!(reg.index_bytes(), before);
}

#[test]
fn unknown_tool_denied_typed_at_slice_open_and_seam() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    assert!(matches!(
        reg.slice(&[("ghost".into(), SliceMode::Allow)])
            .unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
    assert!(matches!(
        reg.tool_open("ghost").unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
    let lens = ToolLens::default();
    assert!(matches!(
        reg.invoke(&OkExec, &lens, "ghost", json!({})).unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
}

#[test]
fn ondemand_and_lens_enforced_at_the_seam() {
    // QA-R1 fix 1: OnDemand without tool_open refuses at the seam.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    reg.slice(&[("write".into(), SliceMode::OnDemand)]).unwrap();
    let lens = ToolLens::default();
    assert!(matches!(
        reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
        RegistryError::SchemaNotOpened { .. }
    ));
    reg.tool_open("write").unwrap();
    assert!(reg.invoke(&OkExec, &lens, "write", json!({})).is_ok());
    // QA-R1 fix 2: lens-disabled is uncallable through the single seam.
    let lens = ToolLens {
        disabled_keys: vec!["write".into()],
        ..Default::default()
    };
    assert!(matches!(
        reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
        RegistryError::LensDisabled { .. }
    ));
    assert!(reg.invoke(&OkExec, &lens, "read", json!({})).is_ok());
    // QA-R1 fix 3: invalid disable pattern denies typed at the seam.
    let bad = ToolLens {
        disabled_keys: vec!["*read".into()],
        ..Default::default()
    };
    assert!(matches!(
        reg.invoke(&OkExec, &bad, "read", json!({})).unwrap_err(),
        RegistryError::Denied { .. }
    ));
}

#[test]
fn deny_listed_refused_at_the_seam_and_reslice_releases() {
    // QA-R2 N1: deny-sliced tools refuse typed at the seam (not just
    // omitted from rows). N2: re-slice is last-wins, rows and seam agree.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    assert!(reg
        .slice(&[("write".into(), SliceMode::Deny)])
        .unwrap()
        .is_empty());
    let lens = ToolLens::default();
    assert!(matches!(
        reg.invoke(&OkExec, &lens, "write", json!({})).unwrap_err(),
        RegistryError::DenyListed { .. }
    ));
    assert!(reg.invoke(&OkExec, &lens, "read", json!({})).is_ok());
    let rows = reg.slice(&[("write".into(), SliceMode::Allow)]).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(reg.invoke(&OkExec, &lens, "write", json!({})).is_ok());
}

#[test]
fn bundle_submit_bound_to_catalog() {
    // QA-R1 fix 4: ghost tools submit-typed even with review disabled and
    // no prior validate() call.
    let gate =
        BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
            .unwrap();
    assert!(matches!(
        gate.submit("evil", false, &catalog()).unwrap_err(),
        BundleError::UnknownTool { .. }
    ));
    let gate = BundleGate::parse_yaml("review_required: true\nbundles:\n  fs-read:\n    - read\n")
        .unwrap();
    assert!(matches!(
        gate.submit("fs-read", false, &catalog()).unwrap_err(),
        BundleError::ReviewRequired { .. }
    ));
    assert!(gate.submit("fs-read", true, &catalog()).is_ok());
}

#[test]
fn write_detect_and_overflow_tested() {
    assert!(Registry::detect_write_intent("write", Some("a.rs"), ""));
    assert!(Registry::detect_write_intent("runProcess", None, ""));
    assert!(!Registry::detect_write_intent("read", Some("a.rs"), "{}"));
    // QA-R1 fix 5: read-tier escape + new verbs.
    assert!(Registry::detect_write_intent(
        "read",
        Some("/etc/passwd"),
        "{}"
    ));
    assert!(Registry::detect_write_intent(
        "read",
        Some("../secret"),
        "{}"
    ));
    assert!(Registry::detect_write_intent(
        "read",
        None,
        "run chmod +x out"
    ));
    assert!(Registry::detect_write_intent(
        "read",
        None,
        "truncate -s0 log"
    ));
    let many: Vec<_> = (0..40)
        .map(|i| json!({"name": format!("tool{i:02}"), "description": "d".repeat(500)}))
        .collect();
    assert!(matches!(
        Registry::load_manifest(&json!(many).to_string()).unwrap_err(),
        RegistryError::IndexOverflow { .. }
    ));
    // QA-R1 fix 7: total schema store bound.
    let big = json!({"type": "object", "pad": "x".repeat(7000)});
    let many: Vec<_> = (0..64)
        .map(|i| json!({"name": format!("tool{i:02}"), "description": "d", "schema": big}))
        .collect();
    assert!(matches!(
        Registry::load_manifest(&json!(many).to_string()).unwrap_err(),
        RegistryError::SchemaStoreOverflow { .. }
    ));
}

#[test]
fn tier_gate_pairing_unknown_exec_mcp_write() {
    // Unknown→exec pairs with the ACP unknown-fail-closed gate.
    assert_eq!(tier_of("ghost"), ApprovalTier::Exec);
    assert!(studio_core::acp::decide("ghost", "allow-session", None).is_err());
    // MCP=write floor.
    assert_eq!(tier_of("mcp__read"), ApprovalTier::Write);
    // Exec tier always routes through approval at the ACP gate.
    let d = studio_core::acp::decide("runProcess", "allow-session", None).unwrap();
    assert!(matches!(
        d,
        studio_core::acp::Decision::RequireApproval { .. }
    ));
}

#[test]
fn v3_farm_every_hostile_outcome_typed_no_leak_no_network() {
    let farm = FixtureFarm::new();
    assert!(matches!(
        farm.call(FixtureKind::Hang, &json!({}), Duration::from_millis(10))
            .unwrap_err(),
        FixtureError::Timeout { .. }
    ));
    assert!(matches!(
        farm.call(FixtureKind::SecretEcho, &json!({}), Duration::from_secs(1))
            .unwrap_err(),
        FixtureError::Redacted { .. }
    ));
    assert!(matches!(
        farm.call(
            FixtureKind::OrderDependent,
            &json!({"phase": "run"}),
            Duration::from_secs(1)
        )
        .unwrap_err(),
        FixtureError::OrderViolation { .. }
    ));
    assert!(matches!(
        farm.call(
            FixtureKind::EgressAttempt,
            &json!({"target": "h"}),
            Duration::from_secs(1)
        )
        .unwrap_err(),
        FixtureError::EgressDenied { .. }
    ));
    let farm2 = FixtureFarm::new();
    assert!(farm2
        .call(
            FixtureKind::SchemaRugpull,
            &json!({"schema_version": "v1"}),
            Duration::from_secs(1)
        )
        .is_ok());
    assert!(matches!(
        farm2
            .call(
                FixtureKind::SchemaRugpull,
                &json!({"schema_version": "v2"}),
                Duration::from_secs(1)
            )
            .unwrap_err(),
        FixtureError::SchemaMismatch { .. }
    ));
    // Zero secret material: the redactor leaves nothing behind, and a
    // marker elsewhere never clears a dirty verdict (QA-R1 fix 6).
    let (clean, _) = redact_secrets("echo sk-test-0123456789abcdef done");
    assert!(!clean.contains("sk-test"));
    assert!(!contains_secret_material(&clean));
    assert!(contains_secret_material(
        "leak sk-test-0123456789abcdef plus [REDACTED]"
    ));
    assert!(contains_secret_material(
        "key sk_live_0123456789abcdef012345 here"
    ));
}
