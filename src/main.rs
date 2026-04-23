mod cli;
mod daemon;
mod ipc;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ygg", about = "Yggdrazil — AI agent governance engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// One-time repo setup
    Init {
        #[arg(long)]
        rules: Option<PathBuf>,
    },
    /// Launch agent in a managed world
    Run {
        agent: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Agent self-report hook
    Hook {
        #[arg(long)]
        world: String,
        #[arg(long, value_delimiter = ',')]
        files: Vec<String>,
    },
    /// Smart merge flow
    Sync {
        #[arg(long)]
        prune: bool,
    },
    /// TUI dashboard
    Monit,
    /// Daemon management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Internal: run daemon in background
    #[command(name = "_daemon-run", hide = true)]
    DaemonRun {
        repo_root: String,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
    Stop,
}

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        if !dir.pop() {
            return std::env::current_dir().unwrap();
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let root = repo_root();

    match cli.command {
        Commands::Init { rules } => cli::init::run(&root, rules.as_deref()),
        Commands::Run { agent: _, args: _ } => {
            println!("ygg run — not yet implemented");
            Ok(())
        }
        Commands::Hook { world: _, files: _ } => {
            println!("ygg hook — not yet implemented");
            Ok(())
        }
        Commands::Sync { prune: _ } => {
            println!("ygg sync — not yet implemented");
            Ok(())
        }
        Commands::Monit => {
            println!("ygg monit — not yet implemented");
            Ok(())
        }
        Commands::Daemon { action: _ } => {
            println!("ygg daemon — not yet implemented");
            Ok(())
        }
        Commands::DaemonRun { repo_root } => {
            let path = PathBuf::from(repo_root);
            tokio::runtime::Runtime::new()?
                .block_on(daemon::Daemon::run(path))
        }
    }
}
