use clap::{Parser, Subcommand};

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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
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
