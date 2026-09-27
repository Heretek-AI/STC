use clap::{Parser, Subcommand};

mod init;

#[derive(Parser)]
#[command(name = "studio", about = "STC studio-core CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show worktrees + ledger health for a repo
    Status {
        #[arg(long)]
        repo: String,
    },
    /// First-run onboarding wizard generating .env (rootless default, refuses privileged-dev)
    Init {
        #[arg(long, default_value = ".")]
        repo: String,
        #[arg(long, default_value = "rootless")]
        runtime: String,
        #[arg(long, default_value_t = false)]
        confirm_privileged: bool,
        #[arg(long, default_value = "opencode-go")]
        preset: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
        #[arg(long, default_value_t = false)]
        non_interactive: bool,
        /// Attempt a credential live-ping. Fails closed: the CLI ships no
        /// HTTP client yet, so this always errors with an actionable stub
        /// message instead of claiming an unverified Connected.
        #[arg(long, default_value_t = false)]
        ping: bool,
    },
    /// Follow logs (CDC change_log tail)
    Logs {
        #[arg(long)]
        db: String,
        #[arg(long)]
        follow: bool,
    },
    /// Snapshot a database: VACUUM INTO <out-dir>/studio.db.bak.<ts>
    Backup {
        #[arg(long)]
        db: String,
        #[arg(long)]
        out_dir: String,
    },
    /// Export db snapshot + receipts + manifest. Secrets excluded unless
    /// --include-secrets with --vault-dir and --age-recipient (age envelope).
    Export {
        #[arg(long)]
        db: String,
        #[arg(long)]
        out_dir: String,
        #[arg(long, default_value_t = false)]
        include_secrets: bool,
        #[arg(long)]
        vault_dir: Option<String>,
        #[arg(long)]
        age_recipient: Option<String>,
    },
    /// Bring up the compose stack. Prints the privileged-dev banner when the
    /// dev override is selected (loud privilege, never silent).
    Up {
        #[arg(long, default_value_t = false)]
        dev: bool,
        #[arg(long)]
        repo: Option<String>,
    },
    /// Get or set the per-project autonomy dial (advisory ↔ full, default
    /// full). Advisory pauses scope/dispatch/merge for approval; destructive
    /// ops always require approval regardless of mode.
    Autonomy {
        #[arg(long)]
        db: String,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        mode: Option<String>,
    },
    /// Print the read-only projection snapshot as JSON (cockpit browser-dev
    /// `/snapshot` source; UI never owns state).
    Snapshot {
        #[arg(long)]
        db: String,
    },
    /// List native RolePack names (sources of truth for marketplace mirrors).
    Rolepacks,
    /// Mirror all native RolePacks to `roles/<name>/{rolepack.yaml,
    /// rolepack.lock, emitted targets}` under `--dir` (marketplace publish).
    /// Fails closed on the first pack that does not verify clean.
    ExportPacks {
        #[arg(long)]
        dir: String,
    },
    /// Request an Ask-policy approval (prints the approval id; the push leg
    /// travels over the approval-relay transport, decision via ApprovalDecide).
    Approval {
        #[arg(long)]
        db: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        action: String,
        #[arg(long, default_value = "")]
        detail: String,
    },
    /// Record a one-tap approval decision (the phone-tap surface until the
    /// mobile client lands). Double decisions fail closed.
    ApprovalDecide {
        #[arg(long)]
        db: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        decision: String,
        #[arg(long, default_value = "")]
        comment: String,
    },
    /// Store one memory doc through the real write path (deny-list + budgets
    /// enforced). Body should be an anchored summary for L1.
    MemoryPut {
        #[arg(long)]
        db: String,
        #[arg(long)]
        doc_id: String,
        #[arg(long, default_value = "L1")]
        level: String,
        #[arg(long)]
        body: String,
    },
    /// FTS retrieval over the memory store (rung-2 read path). Prints
    /// `doc_id<TAB>body` per hit, best first.
    MemorySearch {
        #[arg(long)]
        db: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 3)]
        limit: usize,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init {
            repo,
            runtime,
            confirm_privileged,
            preset,
            api_key,
            force,
            non_interactive,
            ping,
        } => {
            cmd_init(InitArgs {
                repo,
                runtime,
                confirm_privileged,
                preset,
                api_key,
                force,
                non_interactive,
                ping,
            })
            .await;
        }
        Cmd::Status { repo } => {
            let repo_p = std::path::Path::new(&repo);
            match studio_core::worktree::WorktreeManager::list_worktrees(repo_p).await {
                Ok(list) => {
                    println!("worktrees={}", list.len());
                    for w in list {
                        println!("worktree {w}");
                    }
                }
                Err(e) => {
                    eprintln!("status failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Logs { db, follow } => match studio_core::state::StateStore::open(&db) {
            Ok(s) => {
                let mut from: i64 = 0;
                loop {
                    match s.poll_changes(from, 512) {
                        Ok(rows) => {
                            for (seq, tbl, op) in rows {
                                println!("{seq} {tbl} {op}");
                                from = from.max(seq);
                            }
                        }
                        Err(e) => {
                            eprintln!("poll failed: {e}");
                            std::process::exit(1);
                        }
                    }
                    if !follow {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            Err(e) => {
                eprintln!("open db failed: {e}");
                std::process::exit(1);
            }
        },
        Cmd::Backup { db, out_dir } => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let dest = std::path::Path::new(&out_dir).join(format!("studio.db.bak.{ts}"));
            match studio_core::state::StateStore::open(&db) {
                Ok(s) => match s.backup_to(&dest) {
                    Ok(_) => println!("backup {}", dest.display()),
                    Err(e) => {
                        eprintln!("backup failed: {e}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("open db failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Export {
            db,
            out_dir,
            include_secrets,
            vault_dir,
            age_recipient,
        } => {
            if let Err(e) = export(&db, &out_dir, include_secrets, vault_dir, age_recipient).await {
                eprintln!("export failed: {e}");
                std::process::exit(1);
            }
        }
        Cmd::Up { dev, repo } => cmd_up(dev, repo).await,
        Cmd::Autonomy { db, repo, mode } => cmd_autonomy(&db, &repo, mode),
        Cmd::Snapshot { db } => match studio_core::state::StateStore::open(&db) {
            Ok(s) => match s.snapshot() {
                Ok(snap) => match serde_json::to_string(&snap) {
                    Ok(json) => println!("{json}"),
                    Err(e) => {
                        eprintln!("snapshot serialize failed: {e}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("snapshot failed: {e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("open db failed: {e}");
                std::process::exit(1);
            }
        },
        Cmd::Rolepacks => {
            for p in studio_core::roles::RolePack::all_native_packs() {
                println!(
                    "{} v{} slot={} harness={}",
                    p.name, p.version, p.model_slot, p.harness_profile.harness
                );
            }
        }
        Cmd::ExportPacks { dir } => {
            let catalog: Vec<String> = studio_core::mcp::catalog()
                .into_iter()
                .map(|t| t.id)
                .collect();
            match studio_core::roles::RolePack::mirror_packs(std::path::Path::new(&dir), &catalog) {
                Ok(files) => println!("mirrored {} files under {dir}/roles", files.len()),
                Err(e) => {
                    eprintln!("export packs failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::Approval {
            db,
            task,
            action,
            detail,
        } => match studio_core::state::StateStore::open(&db) {
            Ok(s) => {
                match studio_core::scheduler::autonomy::request_approval(
                    &s, &task, &action, &detail,
                ) {
                    Ok(id) => println!("{id}"),
                    Err(e) => {
                        eprintln!("approval request failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("open db failed: {e}");
                std::process::exit(1);
            }
        },
        Cmd::ApprovalDecide {
            db,
            id,
            decision,
            comment,
        } => {
            let approved = match decision.trim().to_lowercase().as_str() {
                "approve" | "approved" | "yes" => true,
                "deny" | "denied" | "no" => false,
                other => {
                    eprintln!(
                        "invalid decision '{other}': expected approve|deny (refusing to guess)"
                    );
                    std::process::exit(1);
                }
            };
            match studio_core::state::StateStore::open(&db) {
                Ok(s) => {
                    match studio_core::scheduler::autonomy::decide_approval(
                        &s, &id, approved, &comment,
                    ) {
                        Ok(()) => println!("decided {id} approved={approved}"),
                        Err(e) => {
                            eprintln!("approval decide failed: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("open db failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Cmd::MemoryPut {
            db,
            doc_id,
            level,
            body,
        } => match studio_core::memory::MemoryStore::open(&db) {
            Ok(s) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                match s.put(&doc_id, &level, &body, now_ms) {
                    Ok(()) => println!("stored {doc_id}"),
                    Err(e) => {
                        eprintln!("memory put failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("open db failed: {e}");
                std::process::exit(1);
            }
        },
        Cmd::MemorySearch { db, query, limit } => {
            match studio_core::memory::MemoryStore::open(&db) {
                Ok(s) => match s.search_fts(&query, limit) {
                    Ok(hits) => {
                        for (id, body) in hits {
                            println!("{id}\t{body}");
                        }
                    }
                    Err(e) => {
                        eprintln!("memory search failed: {e}");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    eprintln!("open db failed: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn cmd_autonomy(db: &str, repo: &str, mode: Option<String>) {
    let store = match studio_core::state::StateStore::open(db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("open db failed: {e}");
            std::process::exit(1);
        }
    };
    match mode {
        None => match store.get_autonomy(repo) {
            Ok(m) => println!("autonomy {repo} {}", m.as_str()),
            Err(e) => {
                eprintln!("get autonomy failed: {e}");
                std::process::exit(1);
            }
        },
        Some(m) => {
            let parsed = match m.trim().to_lowercase().as_str() {
                "advisory" => studio_core::scheduler::autonomy::Autonomy::Advisory,
                "full" => studio_core::scheduler::autonomy::Autonomy::Full,
                other => {
                    eprintln!(
                        "invalid mode '{other}': expected 'advisory' or 'full' (refusing to guess)"
                    );
                    std::process::exit(1);
                }
            };
            if let Err(e) = store.set_autonomy(repo, parsed) {
                eprintln!("set autonomy failed: {e}");
                std::process::exit(1);
            }
            let _ = store.append_event(
                "autonomy.set",
                &serde_json::json!({"repo": repo, "mode": parsed.as_str()}).to_string(),
            );
            println!("autonomy {repo} {}", parsed.as_str());
        }
    }
}

async fn export(
    db: &str,
    out_dir: &str,
    include_secrets: bool,
    vault_dir: Option<String>,
    age_recipient: Option<String>,
) -> Result<(), String> {
    let out = std::path::Path::new(out_dir);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let store = studio_core::state::StateStore::open(db).map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    store
        .backup_to(&out.join(format!("studio.db.bak.{ts}")))
        .map_err(|e| e.to_string())?;
    let receipts = store.export_receipts().map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("receipts.json"),
        serde_json::to_string_pretty(&receipts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut manifest = serde_json::json!({
        "ts": ts,
        "schema_version": studio_core::state::SCHEMA_VERSION,
        "secrets_included": false,
    });
    if include_secrets {
        let vdir = vault_dir.ok_or_else(|| "--include-secrets requires --vault-dir".to_string())?;
        let recipient = age_recipient
            .ok_or_else(|| "--include-secrets requires --age-recipient".to_string())?;
        // Envelope the at-rest-encrypted vault files (never plaintext) via age.
        let tar = tokio::process::Command::new("tar")
            .args(["-czf", "-", "-C", &vdir, "."])
            .stdout(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("tar failed (is tar installed?): {e}"))?;
        if !tar.status.success() {
            return Err("tar of vault dir failed".into());
        }
        let mut age_cmd = tokio::process::Command::new("age");
        age_cmd
            .args(["-r", &recipient, "-o"])
            .arg(out.join("secrets.age"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null());
        let mut child = age_cmd
            .spawn()
            .map_err(|e| format!("age failed (is age installed?): {e}"))?;
        use tokio::io::AsyncWriteExt;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "age stdin unavailable".to_string())?
            .write_all(&tar.stdout)
            .await
            .map_err(|e| e.to_string())?;
        drop(child.stdin.take());
        let st = child.wait().await.map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("age encryption failed".into());
        }
        manifest["secrets_included"] = serde_json::json!(true);
        manifest["secrets_envelope"] = serde_json::json!("age");
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("export {}", out.display());
    Ok(())
}

async fn cmd_up(dev: bool, repo: Option<String>) {
    // Find the stack root by walking up to a dir containing compose.yaml.
    let mut stack_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    loop {
        if stack_dir.join("compose.yaml").exists() {
            break;
        }
        match stack_dir.parent() {
            Some(p) => stack_dir = p.to_path_buf(),
            None => {
                eprintln!("studio up: no compose.yaml found walking up from current dir");
                std::process::exit(1);
            }
        }
    }
    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        stack_dir.join("compose.yaml").to_string_lossy().to_string(),
    ];
    if dev {
        println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
        println!("STUDIO DEV-MODE: socket-mounted manager — lanes are ROOT on");
        println!("this host. Local dev only. Never with untrusted prompts or");
        println!("on shared machines. (compose.override.dev.yaml)");
        println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
        args.push("-f".to_string());
        args.push("compose.override.dev.yaml".to_string());
    }
    args.push("up".to_string());
    args.push("-d".to_string());
    if let Some(r) = repo {
        std::env::set_var("STUDIO_REPO", r);
    }
    let st = tokio::process::Command::new("docker")
        .args(&args)
        .status()
        .await;
    match st {
        Ok(s) if s.success() => println!("studio up (dev={dev})"),
        _ => {
            eprintln!("studio up failed: is docker installed and running?");
            std::process::exit(1);
        }
    }
}

/// Grouped `studio init` arguments (keeps `cmd_init` under the arg-count lint).
struct InitArgs {
    repo: String,
    runtime: String,
    confirm_privileged: bool,
    preset: String,
    api_key: Option<String>,
    force: bool,
    non_interactive: bool,
    ping: bool,
}

async fn cmd_init(args: InitArgs) {
    let InitArgs {
        repo,
        runtime,
        mut confirm_privileged,
        preset,
        mut api_key,
        force,
        non_interactive,
        ping,
    } = args;
    let repo_path = std::path::PathBuf::from(&repo);
    let mut chosen_runtime = match init::LaneRuntime::parse(&runtime) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("init error: {e}");
            std::process::exit(1);
        }
    };

    if !non_interactive {
        use std::io::{stdin, stdout, Write};
        println!("=== STC First-Run Setup Wizard ===");
        println!("Target repository path: {}", repo_path.display());

        print!("Choose lane runtime [rootless (default) | privileged-dev]: ");
        let _ = stdout().flush();
        let mut line = String::new();
        if stdin().read_line(&mut line).is_ok() && !line.trim().is_empty() {
            match init::LaneRuntime::parse(&line) {
                Ok(r) => chosen_runtime = r,
                Err(e) => {
                    eprintln!("Invalid choice: {e}");
                    std::process::exit(1);
                }
            }
        }

        if chosen_runtime == init::LaneRuntime::PrivilegedDev && !confirm_privileged {
            println!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
            println!("WARNING: privileged-dev grants containers root access to host via socket.");
            println!("Local development only. Never run with untrusted prompts.");
            println!("Type 'I UNDERSTAND THE RISKS' to confirm:");
            print!("> ");
            let _ = stdout().flush();
            let mut conf = String::new();
            if stdin().read_line(&mut conf).is_ok() {
                let trimmed = conf.trim();
                if trimmed == "I UNDERSTAND THE RISKS" || trimmed == "yes" {
                    confirm_privileged = true;
                    println!("privileged-dev confirmed.");
                } else {
                    eprintln!("Refusing privileged-dev: confirmation did not match. Aborting.");
                    std::process::exit(1);
                }
            }
        }

        if api_key.is_none() {
            print!("Enter OPENCODE_API_KEY (optional, press Enter to skip): ");
            let _ = stdout().flush();
            let mut k = String::new();
            if stdin().read_line(&mut k).is_ok() && !k.trim().is_empty() {
                api_key = Some(k.trim().to_string());
            }
        }
    }

    let model_preset = init::ModelPreset::parse(&preset);
    if ping {
        eprintln!("init failed: {}", model_preset.live_ping_stub_error());
        std::process::exit(1);
    }
    let has_key = api_key.as_deref().is_some_and(|k| !k.trim().is_empty());
    let config = init::InitConfig {
        repo_path,
        runtime: chosen_runtime,
        privileged_confirmed: confirm_privileged,
        model_preset,
        api_key,
    };

    let rendered = match init::EnvWriter::validate_and_render(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("init failed: {e}");
            std::process::exit(1);
        }
    };

    let dest = std::path::Path::new(".env");
    match init::EnvWriter::write_env_file(dest, &rendered, force) {
        Ok(_) => {
            println!(
                "Successfully generated .env (runtime={})",
                chosen_runtime.as_str()
            );
            if has_key {
                println!(
                    "Credential written but NOT live-pinged: verify it in the cockpit Providers UI."
                );
            }
            println!("Next step: run `studio up` to start the lane runtime.");
        }
        Err(e) => {
            eprintln!("Failed to write .env: {e}");
            std::process::exit(1);
        }
    }
}
