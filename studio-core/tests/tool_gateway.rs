//! Phase-03 acceptance (harden-03-gateway full rework): registry-held lens
//! (no lens param), catalog-bound handles (no caller catalog param),
//! closed-by-default slicing with versioned receipts, canonical containment
//! with bound root, single ordered egress, and the V3 fixture farm through
//! the seam. Every vector has a dedicated test: forged-lens,
//! forged-catalog, containment matrix, unsliced-deny, seam-order.

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

fn fixture_manifest() -> String {
    json!([
        {"name": "fixture_honest", "description": "honest control", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}},
        {"name": "fixture_hang", "description": "hang fixture", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}},
        {"name": "fixture_secret_echo", "description": "secret echo", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}},
        {"name": "fixture_order_dependent", "description": "order fixture", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}},
        {"name": "fixture_egress_attempt", "description": "egress fixture", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}},
        {"name": "fixture_schema_rugpull", "description": "rugpull fixture", "version": "1.0.0",
         "tags": ["test"], "schema": {"type": "object"}}
    ])
    .to_string()
}

struct OkExec;
impl ToolExecutor for OkExec {
    fn execute(&self, tool: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(json!({"tool": tool}))
    }
}

fn slice(reg: &mut Registry, modes: &[(&str, SliceMode)]) -> SliceReceipt {
    let v: Vec<(String, SliceMode)> = modes.iter().map(|(n, m)| ((*n).into(), *m)).collect();
    reg.slice(&v).unwrap()
}

#[test]
fn registry_loads_from_manifest() {
    let reg = Registry::load_manifest(&manifest()).unwrap();
    assert_eq!(reg.len(), 3);
    assert!(reg.contains("read"));
    assert_eq!(reg.generation(), 1);
}

#[test]
fn slicing_allow_ondemand_deny_holds() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = reg
        .slice(&[
            ("read".into(), SliceMode::Allow),
            ("write".into(), SliceMode::OnDemand),
            ("runProcess".into(), SliceMode::Deny),
        ])
        .unwrap();
    assert_eq!(receipt.rows().len(), 2);
    assert!(!receipt.rows()[0].on_demand);
    assert!(receipt.rows().iter().any(|r| r.on_demand));
}

#[test]
fn lazy_schema_load_keeps_index_bounded() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let before = reg.index_bytes();
    assert!(before <= MAX_INDEX_BYTES);
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let schema = reg.tool_open("read", &receipt).unwrap();
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
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    assert!(matches!(
        reg.tool_open("ghost", &receipt).unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
    assert!(matches!(
        reg.invoke(&OkExec, "ghost", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
}

#[test]
fn ondemand_and_held_lens_enforced_at_the_seam() {
    // OnDemand without tool_open refuses at the seam.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = slice(&mut reg, &[("write", SliceMode::OnDemand)]);
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::SchemaNotOpened { .. }
    ));
    reg.tool_open("write", &receipt).unwrap();
    assert!(reg.invoke(&OkExec, "write", json!({}), &receipt).is_ok());
    // Held lens disabled is uncallable through the single seam (no param).
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    reg.rotate_lens(
        ToolLens {
            disabled_keys: vec!["write".into()],
            ..Default::default()
        },
        "test".into(),
    )
    .unwrap();
    let receipt = slice(&mut reg, &[("write", SliceMode::Allow)]);
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::LensDisabled { .. }
    ));
    // Invalid lens rotation refuses typed and changes nothing.
    let bad = ToolLens {
        disabled_keys: vec!["*read".into()],
        ..Default::default()
    };
    assert!(matches!(
        reg.rotate_lens(bad, "bad".into()).unwrap_err(),
        RegistryError::Denied { .. }
    ));
}

#[test]
fn deny_listed_refused_at_the_seam_and_reslice_releases() {
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = slice(&mut reg, &[("write", SliceMode::Deny)]);
    assert!(receipt.rows().iter().all(|r| r.name != "write"));
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::DenyListed { .. }
    ));
    // Unsliced sibling is NotSliced (closed-by-default), not Ok.
    assert!(matches!(
        reg.invoke(&OkExec, "read", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::NotSliced { .. }
    ));
    let receipt2 = slice(&mut reg, &[("write", SliceMode::Allow)]);
    assert_eq!(
        receipt2.rows().iter().filter(|r| r.name == "write").count(),
        1
    );
    assert!(reg.invoke(&OkExec, "write", json!({}), &receipt2).is_ok());
}

#[test]
fn bundle_submit_bound_to_catalog_no_caller_param() {
    // Ghost tools refuse typed via the HELD catalog; there is no caller
    // catalog parameter to forge.
    let gate =
        BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
            .unwrap();
    let reg = Registry::load_manifest(&manifest()).unwrap();
    let handle = reg.catalog_handle();
    assert!(matches!(
        reg.submit_bundle(&gate, "evil", false, &handle)
            .unwrap_err(),
        BundleError::UnknownTool { .. }
    ));
    let gate = BundleGate::parse_yaml("review_required: true\nbundles:\n  fs-read:\n    - read\n")
        .unwrap();
    assert!(matches!(
        reg.submit_bundle(&gate, "fs-read", false, &handle)
            .unwrap_err(),
        BundleError::ReviewRequired { .. }
    ));
    assert!(reg.submit_bundle(&gate, "fs-read", true, &handle).is_ok());
}

#[test]
fn forged_lens_unrepresentable_and_rotate_audited() {
    // V03-A1: invoke has no lens parameter (compile-time). The only lens is
    // the held one; rotation is the sole mutation and it audits.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let seq = reg
        .rotate_lens(
            ToolLens {
                disabled_keys: vec!["write".into()],
                ..Default::default()
            },
            "restrict write".into(),
        )
        .unwrap();
    assert_eq!(seq, 1);
    assert_eq!(reg.lens_audit().len(), 1);
    assert_eq!(reg.lens_audit()[0].disabled_keys, vec!["write".to_string()]);
    let receipt = slice(&mut reg, &[("write", SliceMode::Allow)]);
    // Attacker crafts a permissive lens value but has no seam parameter for
    // it — the held restrictive lens still denies.
    let _forged_permissive = ToolLens::default();
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::LensDisabled { .. }
    ));
    // Audit trail grows on the re-allow rotation.
    reg.rotate_lens(ToolLens::default(), "re-allow".into())
        .unwrap();
    assert_eq!(reg.lens_audit().len(), 2);
    assert!(reg.invoke(&OkExec, "write", json!({}), &receipt).is_ok());
}

#[test]
fn forged_catalog_unrepresentable_ghost_submit_refused() {
    // V03-A2: submit takes no caller catalog. A caller vector containing the
    // ghost tool cannot be passed — the only catalog is the held index.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let _forged_catalog: Vec<String> = vec!["ghost-tool".into(), "read".into()];
    // The forged vector has no API to reach the seam; the bound submit
    // validates against the held index and refuses the ghost bundle.
    let gate =
        BundleGate::parse_yaml("review_required: false\nbundles:\n  evil:\n    - ghost-tool\n")
            .unwrap();
    let handle = reg.catalog_handle();
    assert!(matches!(
        reg.submit_bundle(&gate, "evil", false, &handle)
            .unwrap_err(),
        BundleError::UnknownTool { .. }
    ));
    // Honest bundle through the same handle passes.
    let gate2 =
        BundleGate::parse_yaml("review_required: false\nbundles:\n  ok:\n    - read\n").unwrap();
    assert!(reg.submit_bundle(&gate2, "ok", false, &handle).is_ok());
    // Stale handle after reload refuses typed.
    let stale = handle;
    reg.reload(&manifest()).unwrap();
    assert!(matches!(
        reg.submit_bundle(&gate2, "ok", false, &stale).unwrap_err(),
        BundleError::StaleHandle { .. }
    ));
    // Old slice receipt equally stale at invoke.
    assert!(matches!(
        reg.invoke(&OkExec, "read", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::StaleHandle { .. }
    ));
}

#[test]
fn containment_matrix_canonical_with_bound_root() {
    // V03-B: canonical containment matrix over a bound temp root.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    reg.bind_root(dir.path()).unwrap();
    let root = reg.root().to_path_buf();
    // Allow: relative inside, normalized-inside.
    assert!(reg.check_containment("read", Some("docs/guide.md")).is_ok());
    assert!(reg.check_containment("read", Some("a/./b/../c")).is_ok());
    // Deny: .. escape, absolute outside, prefix-sibling (substring but not
    // component-wise within root).
    assert!(reg.check_containment("read", Some("../outside")).is_err());
    assert!(reg.check_containment("read", Some("/etc/passwd")).is_err());
    let sibling = format!("{}-evil/file", root.display());
    assert!(reg.check_containment("read", Some(&sibling)).is_err());
    // Seam enforces the same: escape-shaped read refuses containment/tier
    // typed, never Ok.
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    assert!(reg
        .invoke(&OkExec, "read", json!({"path": "../evil"}), &receipt)
        .is_err());
    assert!(reg
        .invoke(&OkExec, "read", json!({"path": "docs/guide.md"}), &receipt)
        .is_ok());
}

#[test]
fn containment_symlink_fold_hidden_traversal_denies_through_seam() {
    // F1 retry: `link/../evil` folds lexically to `root/evil` but traverses
    // `link` on the real FS — must DENY at containment and at the seam.
    #[cfg(unix)]
    {
        let mut reg = Registry::load_manifest(&manifest()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "s").unwrap();
        reg.bind_root(dir.path()).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(reg
            .check_containment("read", Some("link/secret.txt"))
            .is_err());
        assert!(reg.check_containment("read", Some("link/../evil")).is_err());
        assert!(reg
            .check_containment("read", Some("a/../link/../evil2"))
            .is_err());
        let abs_hidden = format!("{}/link/../evil3", reg.root().display());
        assert!(reg.check_containment("read", Some(&abs_hidden)).is_err());
        // Seam: hidden traversal through invoke args also denies.
        let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
        assert!(reg
            .invoke(&OkExec, "read", json!({"path": "link/../evil"}), &receipt)
            .is_err());
    }
}

#[test]
fn receipt_binding_no_desync_old_receipt_never_widens() {
    // F3 retry: old receipts never widen to later slices/modes; stale
    // `opened` never survives a re-slice without a fresh `tool_open`.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let r_read = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let _r2 = slice(&mut reg, &[("write", SliceMode::Allow)]);
    assert!(reg.invoke(&OkExec, "write", json!({}), &r_read).is_err());
    assert!(reg.tool_open("write", &r_read).is_err());

    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let r_deny = slice(&mut reg, &[("write", SliceMode::Deny)]);
    let _r_allow = slice(&mut reg, &[("write", SliceMode::Allow)]);
    assert!(reg.invoke(&OkExec, "write", json!({}), &r_deny).is_err());

    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let r_od = slice(&mut reg, &[("write", SliceMode::OnDemand)]);
    reg.tool_open("write", &r_od).unwrap();
    let _ = slice(&mut reg, &[("write", SliceMode::Deny)]);
    let r_od2 = slice(&mut reg, &[("write", SliceMode::OnDemand)]);
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &r_od2).unwrap_err(),
        RegistryError::SchemaNotOpened { .. }
    ));
}

struct LeakyErrSeam;
impl ToolExecutor for LeakyErrSeam {
    fn execute(&self, _t: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
        Err("boom sk-test-0123456789abcdef leaked via err".into())
    }
}

#[test]
fn invoke_err_path_redacts_no_secret_in_denied_reason() {
    // F2 retry: `Err` strings are scanned like `Ok`; raw `sk-test` never
    // appears in the typed `Denied` reason.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let err = reg
        .invoke(&LeakyErrSeam, "read", json!({}), &receipt)
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(!err.to_string().contains("sk-test"), "leak: {err:?}");
}

#[test]
fn redactor_families_xoxo_gho_bearer_blocked_through_seam() {
    // F4 retry: new families (`xoxo-`, `gho_`, `Bearer`) are blocked at the
    // seam as `Denied`, never `Ok`.
    struct LeakyXoxo;
    impl ToolExecutor for LeakyXoxo {
        fn execute(&self, _t: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
            Ok(json!({"leak": "xoxo-12345678901234567890"}))
        }
    }
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let receipt = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let err = reg
        .invoke(&LeakyXoxo, "read", json!({}), &receipt)
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(!err.to_string().contains("xoxo-"), "leak: {err:?}");
    assert!(contains_secret_material("gho_12345678901234567890123456"));
    assert!(contains_secret_material(
        "github_pat_12345678901234567890123456"
    ));
    assert!(contains_secret_material("Bearer abcdef1234567890ABCDEF"));
    assert!(!contains_secret_material("flask_app task_manager"));
}

#[test]
fn unsliced_deny_closed_by_default_and_reload_invalidates() {
    // V03-C: fresh registry invokes NOTHING Ok; tool_open equally closed;
    // reload invalidates old receipts.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let empty = reg.slice(&[]).unwrap();
    for tool in ["read", "write", "runProcess"] {
        assert!(matches!(
            reg.invoke(&OkExec, tool, json!({}), &empty).unwrap_err(),
            RegistryError::NotSliced { .. }
        ));
    }
    assert!(matches!(
        reg.tool_open("read", &empty).unwrap_err(),
        RegistryError::NotSliced { .. }
    ));
    // After slicing, invocation works; after reload, the old receipt is
    // stale and the fresh registry is closed again until re-sliced.
    let receipt = slice(
        &mut reg,
        &[("read", SliceMode::Allow), ("write", SliceMode::Allow)],
    );
    assert!(reg.invoke(&OkExec, "read", json!({}), &receipt).is_ok());
    reg.reload(&manifest()).unwrap();
    assert!(matches!(
        reg.invoke(&OkExec, "read", json!({}), &receipt)
            .unwrap_err(),
        RegistryError::StaleHandle { .. }
    ));
    let fresh_empty = reg.slice(&[]).unwrap();
    assert!(matches!(
        reg.invoke(&OkExec, "read", json!({}), &fresh_empty)
            .unwrap_err(),
        RegistryError::NotSliced { .. }
    ));
    let receipt2 = slice(&mut reg, &[("read", SliceMode::Allow)]);
    assert!(reg.invoke(&OkExec, "read", json!({}), &receipt2).is_ok());
}

#[test]
fn seam_order_unknown_coverage_deny_ondemand_lens_tier_executor() {
    // V03-D: single ordered egress.
    // unknown beats stale-coverage.
    let mut reg2 = Registry::load_manifest(&manifest()).unwrap();
    let stale = slice(&mut reg2, &[("read", SliceMode::Allow)]);
    reg2.reload(&manifest()).unwrap();
    assert!(matches!(
        reg2.invoke(&OkExec, "ghost", json!({}), &stale)
            .unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
    // coverage (NotSliced) beats Ok.
    let mut fresh = Registry::load_manifest(&manifest()).unwrap();
    let empty = fresh.slice(&[]).unwrap();
    assert!(matches!(
        fresh
            .invoke(&OkExec, "read", json!({}), &empty)
            .unwrap_err(),
        RegistryError::NotSliced { .. }
    ));
    // deny beats lens: DenyListed wins over LensDisabled.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    reg.rotate_lens(
        ToolLens {
            disabled_keys: vec!["write".into()],
            ..Default::default()
        },
        "d".into(),
    )
    .unwrap();
    let r = slice(&mut reg, &[("write", SliceMode::Deny)]);
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &r).unwrap_err(),
        RegistryError::DenyListed { .. }
    ));
    // ondemand beats lens: SchemaNotOpened wins over LensDisabled.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    reg.rotate_lens(
        ToolLens {
            disabled_keys: vec!["write".into()],
            ..Default::default()
        },
        "d".into(),
    )
    .unwrap();
    let r = slice(&mut reg, &[("write", SliceMode::OnDemand)]);
    assert!(matches!(
        reg.invoke(&OkExec, "write", json!({}), &r).unwrap_err(),
        RegistryError::SchemaNotOpened { .. }
    ));
    // lens beats tier: LensDisabled wins over containment/tier.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    reg.bind_root(dir.path()).unwrap();
    reg.rotate_lens(
        ToolLens {
            disabled_keys: vec!["read".into()],
            ..Default::default()
        },
        "d".into(),
    )
    .unwrap();
    let r = slice(&mut reg, &[("read", SliceMode::Allow)]);
    assert!(matches!(
        reg.invoke(&OkExec, "read", json!({"path": "../evil"}), &r)
            .unwrap_err(),
        RegistryError::LensDisabled { .. }
    ));
    // tier/containment beats executor: escape with permissive lens refuses
    // before the executor's Ok.
    let mut reg = Registry::load_manifest(&manifest()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    reg.bind_root(dir.path()).unwrap();
    let r = slice(&mut reg, &[("read", SliceMode::Allow)]);
    let err = reg
        .invoke(&OkExec, "read", json!({"path": "../evil"}), &r)
        .unwrap_err();
    assert!(
        matches!(err, RegistryError::ContainmentDenied { .. })
            || matches!(err, RegistryError::TierDenied { .. }),
        "expected containment/tier, got {err:?}"
    );
}

/// Executor routing the V3 fixture farm through the single seam.
/// Every hostile outcome maps to a typed seam refusal; secrets never leave
/// as Ok (the seam redaction guard is the second net).
struct FixtureSeamExec {
    farm: FixtureFarm,
}
impl ToolExecutor for FixtureSeamExec {
    fn execute(&self, tool: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        let kind = FixtureFarm::resolve(tool).map_err(|e| e.to_string())?;
        // Hang gets a tight deadline so the test proves the timeout path.
        let deadline = if kind == FixtureKind::Hang {
            Duration::from_millis(10)
        } else {
            Duration::from_secs(1)
        };
        match self.farm.call(kind, &args, deadline) {
            Ok(s) => Ok(json!({"result": s})),
            Err(e) => Err(e.to_string()),
        }
    }
}

struct LeakyExec;
impl ToolExecutor for LeakyExec {
    fn execute(&self, _tool: &str, _a: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(json!({"leak": "sk-test-0123456789abcdef"}))
    }
}

#[test]
fn hostile_farm_through_seam_typed_refusal_timeout_redaction() {
    // V03-D hostile farm through the SINGLE seam: every hostile outcome is
    // a typed refusal; timeout/redaction preserved; zero secret leak.
    let mut reg = Registry::load_manifest(&fixture_manifest()).unwrap();
    let receipt = reg
        .slice(&[
            ("fixture_honest".into(), SliceMode::Allow),
            ("fixture_hang".into(), SliceMode::Allow),
            ("fixture_secret_echo".into(), SliceMode::Allow),
            ("fixture_order_dependent".into(), SliceMode::Allow),
            ("fixture_egress_attempt".into(), SliceMode::Allow),
            ("fixture_schema_rugpull".into(), SliceMode::Allow),
        ])
        .unwrap();
    let exec = FixtureSeamExec {
        farm: FixtureFarm::new(),
    };
    // Honest passes.
    assert!(reg
        .invoke(&exec, "fixture_honest", json!({}), &receipt)
        .is_ok());
    // Hang → timeout-flavoured Denied (never Ok).
    let err = reg
        .invoke(&exec, "fixture_hang", json!({}), &receipt)
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(
        err.to_string().to_lowercase().contains("timed out"),
        "{err:?}"
    );
    // Secret-echo → redaction-flavoured Denied, zero leak.
    let err = reg
        .invoke(&exec, "fixture_secret_echo", json!({}), &receipt)
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(!err.to_string().contains("sk-test-"), "leak: {err:?}");
    // Order violation → Denied mentioning order.
    let err = reg
        .invoke(
            &exec,
            "fixture_order_dependent",
            json!({"phase": "run"}),
            &receipt,
        )
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(err.to_string().to_lowercase().contains("order"), "{err:?}");
    // Egress attempt → Denied mentioning egress/target.
    let err = reg
        .invoke(
            &exec,
            "fixture_egress_attempt",
            json!({"target": "https://evil.example/x"}),
            &receipt,
        )
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    // Rugpull: pin v1 ok, then v2 malicious → Denied.
    assert!(reg
        .invoke(
            &exec,
            "fixture_schema_rugpull",
            json!({"schema_version": "v1"}),
            &receipt
        )
        .is_ok());
    let err = reg
        .invoke(
            &exec,
            "fixture_schema_rugpull",
            json!({"schema_version": "v2-malicious"}),
            &receipt,
        )
        .unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    // Unknown fixture through the seam → UnknownTool (fail-closed).
    assert!(matches!(
        reg.invoke(&exec, "ghost", json!({}), &receipt).unwrap_err(),
        RegistryError::UnknownTool { .. }
    ));
    // Leaky executor Ok with secret → seam redaction guard denies, no leak.
    let mut reg2 = Registry::load_manifest(&manifest()).unwrap();
    let r2 = slice(&mut reg2, &[("read", SliceMode::Allow)]);
    let err = reg2.invoke(&LeakyExec, "read", json!({}), &r2).unwrap_err();
    assert!(matches!(err, RegistryError::Denied { .. }), "{err:?}");
    assert!(!err.to_string().contains("sk-test-"), "leak: {err:?}");
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
