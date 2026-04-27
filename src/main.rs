mod cli;
mod daemon;
mod ipc;
mod tui;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ygg", about = "Yggdrazil — AI agent governance engine", version)]
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
        /// Run agent against a local llama.cpp model instead of a remote API
        #[arg(long)]
        local: bool,
        /// Local model to use. Implies --local. Formats:
        ///   org/repo/file.gguf  — download from HuggingFace if not found locally
        ///   /path/to/model.gguf — direct path
        ///   name                — substring search in local model paths
        #[arg(long, value_name = "SPEC")]
        local_model: Option<String>,
        /// llama-server context window size (tokens). Increase if you hit context errors.
        #[arg(long, default_value = "32768")]
        ctx_size: u32,
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
    /// Check for updates and self-update
    Update,
    /// Manage the local llama-server process
    Llama {
        #[command(subcommand)]
        action: LlamaAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
    Stop,
}

#[derive(Subcommand)]
enum LlamaAction {
    /// Show whether llama-server is running and its details
    Status,
    /// Stop the running llama-server
    Stop,
}

fn validate_world_id(id: &str) -> Result<()> {
    if id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        Ok(())
    } else {
        anyhow::bail!("Invalid world id {:?}: only alphanumeric, '-', and '_' are allowed", id)
    }
}

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        let git_path = dir.join(".git");
        if git_path.is_dir() {
            return dir;
        }
        if git_path.is_file() {
            // Worktree: .git file contains "gitdir: /repo/.git/worktrees/id"
            if let Some(real_root) = resolve_worktree_root(&git_path) {
                return real_root;
            }
        }
        if !dir.pop() {
            return std::env::current_dir().unwrap();
        }
    }
}

fn resolve_worktree_root(git_file: &std::path::Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(git_file).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir: ")?;
    // /repo/.git/worktrees/<id> → parent=worktrees → parent=.git → parent=repo
    PathBuf::from(gitdir).parent()?.parent()?.parent().map(|p| p.to_path_buf())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let root = repo_root();

    match cli.command {
        Commands::Init { rules } => cli::init::run(&root, rules.as_deref()),
        Commands::Run { local, local_model, ctx_size, agent, args } => {
            cli::run::run(&root, &agent, &args, None, local, ctx_size, local_model.as_deref())
        }
        Commands::Hook { world, files } => {
            validate_world_id(&world)?;
            cli::hook::run(&root, &world, &files)
        }
        Commands::Sync { prune } => cli::sync::run(&root, prune),
        Commands::Monit => cli::monit::run(&root),
        Commands::Daemon { action } => match action {
            DaemonAction::Start => cli::daemon_cmd::start(&root),
            DaemonAction::Stop => cli::daemon_cmd::stop(&root),
        },
        Commands::DaemonRun { repo_root } => {
            let path = PathBuf::from(repo_root);
            tokio::runtime::Runtime::new()?
                .block_on(daemon::Daemon::run(path))
        }
        Commands::Update => cli::update::run(),
        Commands::Llama { action } => match action {
            LlamaAction::Status => cli::llama_cmd::status(&root),
            LlamaAction::Stop   => cli::llama_cmd::stop(&root),
        },
    }
}
