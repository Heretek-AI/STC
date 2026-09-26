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
    Status { #[arg(long)] repo: String },
    /// Follow logs (CDC change_log tail)
    Logs {
        #[arg(long)] db: String,
        #[arg(long)] follow: bool,
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
        Cmd::Logs { db, follow } => {
            match studio_core::state::StateStore::open(&db) {
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
            }
        }
    }
}
