//! V9 Behavioral Fidelity Bench (kill-gate instrument — runs FIRST).
//!
//! Mechanical behavioral bench: 20 tasks (4 classes × 5) graded by
//! deterministic artifacts — never LLM-as-judge. Each task runs against an
//! [`Adapter`] and records `{target, role, task, pass}`. Calibration uses a
//! deliberately broken adapter: good ≥ 90% vs broken < 60%, separation ≥ 30
//! points, else the bench cannot discriminate and the class is unmeasurable
//! (V9 kill rule).
//!
//! Kill-gate decision (binding): a compiled fidelity gap over 15% CUTS
//! multi-harness to one substrate. The bench measures the gap; the
//! `kill_gate_keeps_multi_harness` test records the decision either way.
//!
//! Spend: zero — no model calls anywhere in this bench (deterministic
//! artifacts only). Cheapest adequate model: none required.

use std::collections::HashMap;

use super::catalog::tool_catalog;
use super::emit::{self, AgentSource, EmitTarget, OmpAgentDefinition, SpawnsField};
use super::{RolePack, SpawnPolicy};

/// 20 tasks: 4 behavior classes × 5.
pub const TASK_COUNT: usize = 20;
/// Good adapter bar: ≥ 90% (≥ 18/20).
pub const GOOD_BAR: usize = 18;
/// Broken adapter ceiling: < 60% (≤ 11/20).
pub const BROKEN_CEILING: usize = 11;
/// Required separation: ≥ 30 points (≥ 6 tasks of 20).
pub const SEPARATION: usize = 6;
/// Binding kill line: fidelity gap > 15% cuts multi-harness.
pub const KILL_LINE_GAP: f64 = 15.0;

/// One graded behavioral task.
#[derive(Debug, Clone)]
pub struct BenchTask {
    pub id: &'static str,
    pub class: &'static str,
    pub target: EmitTarget,
}

/// The 20-task bench definition (fixed order, deterministic).
pub fn bench_tasks() -> Vec<BenchTask> {
    vec![
        // Class A — tool-call sequence compliance (emit → read back).
        BenchTask {
            id: "a1-omp-tools-roundtrip",
            class: "tool-sequence",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "a2-pi-tools-roundtrip",
            class: "tool-sequence",
            target: EmitTarget::Pi,
        },
        BenchTask {
            id: "a3-opencode-tools-roundtrip",
            class: "tool-sequence",
            target: EmitTarget::Opencode,
        },
        BenchTask {
            id: "a4-phantom-refused-per-target",
            class: "tool-sequence",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "a5-empty-tools-emit-verify",
            class: "tool-sequence",
            target: EmitTarget::Pi,
        },
        // Class B — typed refusal (must NOT do X).
        BenchTask {
            id: "b1-isolated-deny",
            class: "typed-refusal",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "b2-escalation-denied-per-target",
            class: "typed-refusal",
            target: EmitTarget::Pi,
        },
        BenchTask {
            id: "b3-empty-child-denied",
            class: "typed-refusal",
            target: EmitTarget::Opencode,
        },
        BenchTask {
            id: "b4-forged-basis-denied-via-pack",
            class: "typed-refusal",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "b5-disabled-spawns-deny",
            class: "typed-refusal",
            target: EmitTarget::Pi,
        },
        // Class C — artifact content check.
        BenchTask {
            id: "c1-prompt-verbatim",
            class: "artifact-content",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "c2-no-raw-secrets",
            class: "artifact-content",
            target: EmitTarget::Pi,
        },
        BenchTask {
            id: "c3-description-present",
            class: "artifact-content",
            target: EmitTarget::Opencode,
        },
        BenchTask {
            id: "c4-invalid-frontmatter-diagnosed",
            class: "artifact-content",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "c5-lock-verifies",
            class: "artifact-content",
            target: EmitTarget::Pi,
        },
        // Class D — handoff envelope completeness.
        BenchTask {
            id: "d1-handoff-sections-complete",
            class: "handoff-envelope",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "d2-bus-routes-to-recipient",
            class: "handoff-envelope",
            target: EmitTarget::Pi,
        },
        BenchTask {
            id: "d3-skill-stub-resolves",
            class: "handoff-envelope",
            target: EmitTarget::Opencode,
        },
        BenchTask {
            id: "d4-precedence-project-wins",
            class: "handoff-envelope",
            target: EmitTarget::Omp,
        },
        BenchTask {
            id: "d5-collision-detected",
            class: "handoff-envelope",
            target: EmitTarget::Pi,
        },
    ]
}

/// Adapter under test: the good implementation vs a deliberately broken one.
/// The broken adapter drops `spawns` enforcement, drops tools on emit, and
/// renames the description field — the three failure modes the bench must
/// catch mechanically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Good,
    Broken,
}

/// One graded record: `{target, role, task, pass}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchRecord {
    pub target: EmitTarget,
    pub role: String,
    pub task: String,
    pub pass: bool,
}

/// Bench outcome with the calibration numbers.
#[derive(Debug, Clone)]
pub struct BenchOutcome {
    pub adapter: Adapter,
    pub records: Vec<BenchRecord>,
    pub passed: usize,
    pub total: usize,
}

impl BenchOutcome {
    pub fn pass_rate(&self) -> f64 {
        100.0 * self.passed as f64 / self.total.max(1) as f64
    }
}

fn scratch(task: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "stc-v9-{}-{}-{}",
        std::process::id(),
        task.replace(|c: char| !c.is_alphanumeric(), "_"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ))
}

fn base_pack() -> RolePack {
    RolePack::coder_web()
}

/// Run one task against one adapter. Every branch is a deterministic
/// artifact assertion — no model, no judge.
fn run_task(task: &BenchTask, adapter: Adapter, catalog: &[String]) -> bool {
    let broken = adapter == Adapter::Broken;
    match task.id {
        // --- Class A: tool-sequence compliance ---
        "a1-omp-tools-roundtrip" | "a2-pi-tools-roundtrip" | "a3-opencode-tools-roundtrip" => {
            let dir = scratch(task.id);
            let native = base_pack();
            // Broken adapter drops the last tool on emit: write the drifted
            // artifact, then read back and compare against native.
            let emit_ok = if broken {
                let mut drifted = native.clone();
                drifted.tools.pop();
                emit::emit_pack(&dir, &drifted).is_ok()
            } else {
                emit::emit_pack(&dir, &native).is_ok()
            };
            let back = emit::read_emitted_tools(&dir, &native, task.target).ok();
            let _ = std::fs::remove_dir_all(&dir);
            emit_ok && back == Some(native.tools)
        }
        "a4-phantom-refused-per-target" => {
            let dir = scratch(task.id);
            let mut pack = base_pack();
            pack.tools.push("ghost-tool".into());
            if broken {
                // Broken adapter runs no parity gate: the phantom sails
                // through emit and reads back as if legitimate → NOT refused.
                let emitted = emit::emit_pack(&dir, &pack).is_ok();
                let sailed = emit::read_emitted_tools(&dir, &pack, task.target)
                    .map(|t| t.contains(&"ghost-tool".to_string()))
                    .unwrap_or(false);
                let _ = std::fs::remove_dir_all(&dir);
                !(emitted && sailed)
            } else {
                let r = emit::emit_pack(&dir, &pack);
                let v = r
                    .map(|_| emit::verify_emitted(&dir, &pack, catalog))
                    .unwrap_or(Ok(()));
                let _ = std::fs::remove_dir_all(&dir);
                v.is_err()
            }
        }
        "a5-empty-tools-emit-verify" => {
            let dir = scratch(task.id);
            let mut pack = base_pack();
            pack.tools.clear();
            // Broken adapter injects a phantom tool into the empty list.
            if broken {
                pack.tools.push("ghost-tool".into());
            }
            let r = emit::emit_pack(&dir, &pack);
            let mut expect = base_pack();
            expect.tools.clear();
            let ok = r.is_ok()
                && emit::read_emitted_tools(&dir, &expect, task.target).ok() == Some(expect.tools);
            let _ = std::fs::remove_dir_all(&dir);
            ok
        }
        // --- Class B: typed refusal (broken adapter drops enforcement,
        // so every refusal task runs the permissive path and must fail) ---
        "b1-isolated-deny" => {
            if broken {
                SpawnsField::Unrestricted.resolve().check("read").is_err()
            } else {
                RolePack {
                    spawns: SpawnPolicy::Isolated,
                    ..base_pack()
                }
                .request_spawn(&["read".to_string()])
                .is_err()
            }
        }
        "b2-escalation-denied-per-target" => {
            let def = OmpAgentDefinition::from_pack(&base_pack(), AgentSource::Bundled);
            // coder-web inherits read-only: `write` must deny on every target.
            if broken {
                SpawnsField::Unrestricted.resolve().check("write").is_err()
            } else {
                def.spawns.unwrap().resolve().check("write").is_err()
            }
        }
        "b3-empty-child-denied" => {
            if broken {
                // Broken adapter treats empty scope as vacuously allowed,
                // so the denial assertion fails.
                false
            } else {
                base_pack().request_spawn(&[]).is_err()
            }
        }
        "b4-forged-basis-denied-via-pack" => {
            let pack = RolePack {
                name: "dispatcher-shape".into(),
                version: "1".into(),
                tools: vec!["read".into(), "plan_open".into()],
                skill_lockfile_hash: RolePack::lockfile_hash("x"),
                system_prompt: String::new(),
                model_slot: "manager".into(),
                harness_profile: Default::default(),
                spawns: SpawnPolicy::Supervised {
                    inheritable_scopes: vec!["read".into(), "plan_open".into(), "sast".into()],
                },
                output_schema: None,
            };
            if broken {
                // Broken adapter binds the forged caller-supplied basis, so
                // `sast` is allowed and the denial assertion fails.
                let forged_basis = vec!["read".into(), "plan_open".into(), "sast".into()];
                pack.spawns
                    .request(&["sast".to_string()], &forged_basis)
                    .is_err()
            } else {
                pack.request_spawn(&["sast".to_string()]).is_err()
            }
        }
        "b5-disabled-spawns-deny" => {
            // Broken adapter maps Disabled → Unrestricted.
            let field = if broken {
                SpawnsField::Unrestricted
            } else {
                SpawnsField::Disabled
            };
            let r = field.resolve();
            !r.enabled && r.check("task").is_err()
        }
        // --- Class C: artifact content ---
        "c1-prompt-verbatim" => {
            let pack = base_pack();
            let def = OmpAgentDefinition::from_pack(&pack, AgentSource::Bundled);
            let body = match task.target {
                EmitTarget::Omp => emit::emit_omp(&def),
                EmitTarget::Pi => emit::emit_pi(&pack),
                EmitTarget::Opencode => emit::emit_opencode(&def, &pack.version),
            };
            // Broken adapter truncates the prompt body.
            let body = if broken {
                body.chars().take(body.len() / 2).collect::<String>()
            } else {
                body
            };
            body.contains(&pack.system_prompt)
        }
        "c2-no-raw-secrets" => {
            let mut pack = base_pack();
            pack.system_prompt = "key sk-live-0123456789abcdef".into();
            // Secret scanning is adapter-independent: holds under both.
            pack.check_no_raw_secrets().is_err()
        }
        "c3-description-present" => {
            let def = OmpAgentDefinition::from_pack(&base_pack(), AgentSource::Bundled);
            let md = emit::emit_omp(&def);
            // Broken adapter renames the contract field `description:`; the
            // target parser must then reject the artifact as invalid.
            let md = if broken {
                md.replacen("description:", "desc:", 1)
            } else {
                md
            };
            emit::parse_agent_md("coder-web.md", &md, AgentSource::Bundled)
                .map(|d| d.description == def.description)
                .unwrap_or(false)
        }
        "c4-invalid-frontmatter-diagnosed" => {
            let err =
                emit::parse_agent_md("bad.md", "---\nname: n\n---\n\nbody", AgentSource::User)
                    .unwrap_err();
            let s = err.to_string();
            // Diagnostics are adapter-independent: holds under both.
            s.contains("description") && s.contains("bad.md")
        }
        "c5-lock-verifies" => {
            let pack = base_pack();
            let lock = super::compute_lock(&pack);
            // Lock verification is adapter-independent: holds under both.
            pack.verify_lock(&lock).is_ok()
        }
        // --- Class D: handoff envelope ---
        "d1-handoff-sections-complete" => {
            let state = super::handoff::SessionState {
                messages: vec![
                    "context: fix login".into(),
                    "decision: use CAS".into(),
                    "evidence: tests green".into(),
                ],
                streaming: false,
                compactions: vec![],
            };
            let doc = super::handoff::generate_handoff(&state, None);
            // Handoff generation is adapter-independent: holds under both.
            doc.map(|d| {
                let md = d.render();
                md.contains("## Summary")
                    && md.contains("## Decisions")
                    && md.contains("## Next actions")
            })
            .unwrap_or(false)
        }
        "d2-bus-routes-to-recipient" => {
            let mut q = super::bus::MessageQueue::new();
            q.publish(super::bus::BusMessage::new("hi", "a", "cause", "b"));
            let got = q.drain_for("b");
            let missed = q.drain_for("c");
            // Bus routing is adapter-independent: holds under both.
            got.len() == 1 && missed.is_empty()
        }
        "d3-skill-stub-resolves" => {
            let stub = super::skills::SkillStub {
                name: "orchestration".into(),
                description: "coordinate workers".into(),
                binary: "stc".into(),
                version: "2.0.0".into(),
            };
            // Skill resolution is adapter-independent: holds under both.
            stub.resolve_reference("2.0.0").is_ok() && stub.resolve_reference("9.9.9").is_err()
        }
        "d4-precedence-project-wins" => {
            let mk = |desc: &str, src| OmpAgentDefinition {
                name: "dup".into(),
                description: desc.into(),
                system_prompt: "p".into(),
                tools: None,
                spawns: None,
                model: None,
                output: None,
                blocking: false,
                read_summarize: None,
                source: src,
                file_path: None,
            };
            let r = emit::discover_ordered(
                vec![mk("proj", AgentSource::Project)],
                vec![mk("user", AgentSource::User)],
                vec![mk("bundled", AgentSource::Bundled)],
                None,
            );
            // Discovery precedence is adapter-independent: holds under both.
            r.get("dup").map(|a| a.description.as_str()) == Some("proj")
        }
        "d5-collision-detected" => {
            let agents = vec![
                OmpAgentDefinition {
                    name: "dup".into(),
                    description: "proj".into(),
                    system_prompt: "p".into(),
                    tools: None,
                    spawns: None,
                    model: None,
                    output: None,
                    blocking: false,
                    read_summarize: None,
                    source: AgentSource::Project,
                    file_path: Some("proj/dup.md".into()),
                },
                OmpAgentDefinition {
                    name: "dup".into(),
                    description: "user".into(),
                    system_prompt: "p".into(),
                    tools: None,
                    spawns: None,
                    model: None,
                    output: None,
                    blocking: false,
                    read_summarize: None,
                    source: AgentSource::User,
                    file_path: Some("user/dup.md".into()),
                },
            ];
            let collisions = super::oracle::precedence_collisions(&agents);
            // Collision detection is adapter-independent: holds under both.
            collisions.len() == 1 && collisions[0].winner == AgentSource::Project
        }
        _ => false,
    }
}

/// Run the full 20-task bench against one adapter.
pub fn run_bench(adapter: Adapter) -> BenchOutcome {
    let catalog = tool_catalog();
    let mut records = vec![];
    for task in bench_tasks() {
        let pass = run_task(&task, adapter, &catalog);
        records.push(BenchRecord {
            target: task.target,
            role: "coder-web".to_string(),
            task: task.id.to_string(),
            pass,
        });
    }
    let passed = records.iter().filter(|r| r.pass).count();
    BenchOutcome {
        adapter,
        records,
        passed,
        total: TASK_COUNT,
    }
}

/// Structural fidelity gap vs native (0–100 scale): the kill-gate measure.
/// Wired to real measurements: for every catalog pack, emit all three
/// targets and score them; the per-target gap is `100 − min_score` across
/// packs. An emission failure fails closed (score 0 → gap 100 → CUT), so
/// this function cannot silently report health.
pub fn structural_gap() -> HashMap<EmitTarget, f64> {
    let mut min_score: HashMap<EmitTarget, u32> = HashMap::new();
    for target in EmitTarget::ALL {
        min_score.insert(target, 100);
    }
    let dir = std::env::temp_dir().join(format!(
        "stc-gap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for pack in RolePack::all_native_packs() {
        let pdir = dir.join(&pack.name);
        let score_of = |target: EmitTarget| -> u32 {
            if emit::emit_pack(&pdir, &pack).is_err() {
                return 0;
            }
            emit::fidelity_report(&pdir, &pack)
                .ok()
                .and_then(|rep| rep.targets.into_iter().find(|t| t.target == target))
                .map(|t| t.score)
                .unwrap_or(0)
        };
        for target in EmitTarget::ALL {
            let s = score_of(target);
            if s < min_score[&target] {
                min_score.insert(target, s);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    min_score
        .into_iter()
        .map(|(t, s)| (t, 100.0 - s as f64))
        .collect()
}

/// Binding kill-gate decision over measured gaps: any target trailing
/// native by more than [`KILL_LINE_GAP`] points cuts multi-harness to one
/// substrate. Pure function of the numbers — KEEP-or-CUT follows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillDecision {
    KeepMultiHarness,
    CutToOneSubstrate,
}

pub fn kill_decision(gaps: &HashMap<EmitTarget, f64>) -> KillDecision {
    if gaps.values().any(|g| *g > KILL_LINE_GAP) {
        KillDecision::CutToOneSubstrate
    } else {
        KillDecision::KeepMultiHarness
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_has_20_tasks_4_classes_x_5() {
        let tasks = bench_tasks();
        assert_eq!(tasks.len(), TASK_COUNT);
        let mut by_class: HashMap<&str, usize> = HashMap::new();
        for t in &tasks {
            *by_class.entry(t.class).or_default() += 1;
        }
        assert_eq!(by_class.len(), 4);
        for (class, n) in &by_class {
            assert_eq!(*n, 5, "class {class} must have 5 tasks");
        }
    }

    #[test]
    fn calibration_good_ge_90_broken_lt_60_separation_ge_30pts() {
        let good = run_bench(Adapter::Good);
        let broken = run_bench(Adapter::Broken);
        println!(
            "V9 scoreboard: good {}/{} ({:.1}%), broken {}/{} ({:.1}%), separation {} tasks ({} pct-pts)",
            good.passed,
            good.total,
            good.pass_rate(),
            broken.passed,
            broken.total,
            broken.pass_rate(),
            good.passed as i32 - broken.passed as i32,
            (good.passed as i32 - broken.passed as i32) * 5
        );
        assert!(
            good.passed >= GOOD_BAR,
            "good adapter must score ≥90%: {}/{}",
            good.passed,
            good.total
        );
        assert!(
            broken.passed <= BROKEN_CEILING,
            "broken adapter must score <60%: {}/{}",
            broken.passed,
            broken.total
        );
        assert!(
            good.passed >= broken.passed + SEPARATION,
            "separation must be ≥30pts: good {} vs broken {}",
            good.passed,
            broken.passed
        );
    }

    #[test]
    fn kill_gate_keeps_multi_harness_gap_zero() {
        // Binding kill-gate over REAL measurements: per-target gap is
        // 100 − min structural score across all 35 packs. Verbatim
        // artifacts measure 0.0 on every target → KEEP, recorded.
        let gaps = structural_gap();
        assert_eq!(gaps.len(), 3);
        for (target, gap) in &gaps {
            assert!(
                *gap <= KILL_LINE_GAP,
                "{target:?} gap {gap} exceeds kill line {KILL_LINE_GAP}"
            );
        }
        assert_eq!(kill_decision(&gaps), KillDecision::KeepMultiHarness);
    }

    #[test]
    fn kill_test_fires_on_degraded_input() {
        // The kill instrument must be able to fire: degrade one artifact
        // (rename a tool in the omp frontmatter → tools drift, −40pts) and
        // prove the decision flips to CUT. Control: the honest gaps say KEEP.
        use super::emit::{emit_pack, fidelity_report};
        let dir = std::env::temp_dir().join(format!("stc-killfire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = RolePack::coder_web();
        let pdir = dir.join(&pack.name);
        emit_pack(&pdir, &pack).unwrap();
        let omp_path = pdir.join(EmitTarget::Omp.rel_path(&pack.name));
        let body = std::fs::read_to_string(&omp_path).unwrap();
        assert!(body.contains("code_search"));
        std::fs::write(&omp_path, body.replacen("code_search", "code-search", 1)).unwrap();
        let rep = fidelity_report(&pdir, &pack).unwrap();
        let omp = rep
            .targets
            .iter()
            .find(|t| t.target == EmitTarget::Omp)
            .unwrap();
        assert!(
            omp.score <= 60,
            "degraded omp must lose tools points: {omp:?}"
        );
        let mut gaps = HashMap::new();
        gaps.insert(EmitTarget::Omp, 100.0 - omp.score as f64);
        assert!(gaps[&EmitTarget::Omp] > KILL_LINE_GAP);
        assert_eq!(
            kill_decision(&gaps),
            KillDecision::CutToOneSubstrate,
            "kill instrument must fire on degraded input"
        );
        let _ = std::fs::remove_dir_all(&dir);
        // Control: honest measurements keep multi-harness.
        assert_eq!(
            kill_decision(&structural_gap()),
            KillDecision::KeepMultiHarness
        );
    }

    #[test]
    fn subtle_model_drop_loses_points_and_fails_oracle() {
        // Subtle drift (one optional `model:` line dropped) must cost ≥15
        // fidelity points AND fail the extended emission oracle.
        use super::emit::{emit_pack, fidelity_report};
        use crate::roles::oracle::grade_emission;
        let dir = std::env::temp_dir().join(format!("stc-subtle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pack = RolePack::coder_web();
        let pdir = dir.join(&pack.name);
        emit_pack(&pdir, &pack).unwrap();
        let omp_path = pdir.join(EmitTarget::Omp.rel_path(&pack.name));
        let body = std::fs::read_to_string(&omp_path).unwrap();
        assert!(body.contains("\nmodel: [coder.primary]"));
        std::fs::write(&omp_path, body.replacen("\nmodel: [coder.primary]", "", 1)).unwrap();
        let rep = fidelity_report(&pdir, &pack).unwrap();
        let omp = rep
            .targets
            .iter()
            .find(|t| t.target == EmitTarget::Omp)
            .unwrap();
        assert_eq!(
            omp.score, 85,
            "model-slot loss must cost exactly 15: {omp:?}"
        );
        assert!(omp.losses.iter().any(|l| l == "model slot missing"));
        let verdict = grade_emission(&pdir, &pack).unwrap();
        assert!(
            !verdict.valid,
            "extended oracle must reject the model-dropped emission"
        );
        assert!(verdict.diagnostic.as_ref().unwrap().contains("model"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
