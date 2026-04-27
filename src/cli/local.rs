use anyhow::{Context, Result};
use dialoguer::Select;
use std::path::{Path, PathBuf};

const MODEL_SEARCH_PATHS: &[&str] = &[
    "~/.cache/llama.cpp",
    "~/.local/share/llama.cpp",
    "~/models",
    "~/llama.cpp/models",
    "~/.lm-studio/models",
    "/opt/models",
    "/usr/local/share/models",
];

#[derive(Debug, Clone)]
pub struct LocalModel {
    pub name: String,
    pub path: PathBuf,
    pub size_gb: f64,
}

pub struct LocalSetup {
    pub model: LocalModel,
    pub server_port: u16,
    pub env_vars: Vec<(String, String)>,
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn collect_gguf_files(dir: &Path, out: &mut Vec<LocalModel>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gguf_files(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("gguf") {
            let size_gb = std::fs::metadata(&path)
                .map(|m| m.len() as f64 / 1_073_741_824.0)
                .unwrap_or(0.0);
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            out.push(LocalModel { name, path, size_gb });
        }
    }
}

pub fn find_models() -> Vec<LocalModel> {
    let mut models = Vec::new();
    for search_path in MODEL_SEARCH_PATHS {
        let path = expand_home(search_path);
        if path.exists() {
            collect_gguf_files(&path, &mut models);
        }
    }
    // Largest first
    models.sort_by(|a, b| b.size_gb.partial_cmp(&a.size_gb).unwrap_or(std::cmp::Ordering::Equal));
    models
}

/// Available RAM in GB via sysinfo. Returns 8.0 as a safe default if detection fails.
pub fn available_ram_gb() -> f64 {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let bytes = sys.available_memory();
    if bytes == 0 {
        // Fallback: try total memory / 2, or default 8GB
        let total = sys.total_memory();
        if total > 0 { total as f64 / 2.0 / 1_073_741_824.0 } else { 8.0 }
    } else {
        bytes as f64 / 1_073_741_824.0
    }
}

/// Find llama-server in PATH
pub fn find_llama_server() -> Option<PathBuf> {
    for candidate in &["llama-server", "llama-cpp-server", "server"] {
        let out = std::process::Command::new("which")
            .arg(candidate)
            .output()
            .ok()?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

fn find_free_port(base: u16) -> u16 {
    for port in base..base + 100 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    base
}

fn recommend_idx(models: &[LocalModel], ram_gb: f64) -> usize {
    let target_gb = ram_gb * 0.6;
    models
        .iter()
        .position(|m| m.size_gb <= target_gb)
        .unwrap_or(models.len().saturating_sub(1))
}

fn recommend_model_name(ram_gb: f64) -> &'static str {
    if ram_gb >= 32.0 {
        "Llama-3.1-70B-Q4_K_M.gguf"
    } else if ram_gb >= 16.0 {
        "Llama-3.1-13B-Q4_K_M.gguf"
    } else if ram_gb >= 8.0 {
        "Llama-3.1-8B-Q4_K_M.gguf"
    } else {
        "Llama-3.2-3B-Q4_K_M.gguf"
    }
}

pub fn agent_env_vars(agent: &str, server_port: u16, model_name: &str) -> Vec<(String, String)> {
    let base_url = format!("http://localhost:{server_port}/v1");
    match agent {
        "codex" => vec![
            ("OPENAI_BASE_URL".into(), base_url),
            ("OPENAI_API_KEY".into(), "llama-local".into()),
            ("OPENAI_MODEL".into(), model_name.to_string()),
        ],
        "claude" | "claude-code" => vec![
            (
                "ANTHROPIC_BASE_URL".into(),
                format!("http://localhost:{server_port}"),
            ),
            ("ANTHROPIC_MODEL".into(), model_name.to_string()),
        ],
        "aider" => vec![
            ("OPENAI_API_BASE".into(), base_url),
            ("OPENAI_API_KEY".into(), "llama-local".into()),
        ],
        _ => vec![
            ("OPENAI_BASE_URL".into(), base_url),
            ("OPENAI_API_KEY".into(), "llama-local".into()),
        ],
    }
}

pub fn setup(agent: &str) -> Result<LocalSetup> {
    let models = find_models();
    let ram_gb = available_ram_gb();

    if models.is_empty() {
        anyhow::bail!(
            "No GGUF models found in common locations.\n\
             Download a model to ~/models/ or ~/.cache/llama.cpp/\n\
             Recommended for {:.0}GB RAM: {}",
            ram_gb,
            recommend_model_name(ram_gb)
        );
    }

    let model = if models.len() == 1 {
        models.into_iter().next().unwrap()
    } else {
        let rec_idx = recommend_idx(&models, ram_gb);
        let labels: Vec<String> = models
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let tag = if i == rec_idx { " ★" } else { "" };
                format!("{}{} ({:.1}GB)", m.name, tag, m.size_gb)
            })
            .collect();

        println!("Available RAM: {:.1}GB", ram_gb);
        let idx = Select::new()
            .with_prompt("Select local model")
            .items(&labels)
            .default(rec_idx)
            .interact()?;

        models.into_iter().nth(idx).unwrap()
    };

    let server_port = find_free_port(8080);
    let env_vars = agent_env_vars(agent, server_port, &model.name);

    Ok(LocalSetup {
        model,
        server_port,
        env_vars,
    })
}

fn wait_for_server(port: u16) -> Result<()> {
    let url = format!("http://localhost:{port}/health");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("llama-server did not become ready within 30s on port {port}");
        }
        if let Ok(resp) = ureq::get(&url).call() {
            if resp.status() == 200 {
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub fn start_server(server_bin: &Path, model: &LocalModel, port: u16) -> Result<std::process::Child> {
    let child = std::process::Command::new(server_bin)
        .args([
            "--model",
            model.path.to_str().context("model path is not valid UTF-8")?,
            "--port",
            &port.to_string(),
            "--ctx-size",
            "4096",
            "-ngl",
            "99",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to start llama-server")?;

    wait_for_server(port)?;
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_env_vars_codex() {
        let vars = agent_env_vars("codex", 8080, "llama3");
        assert!(vars.iter().any(|(k, v)| k == "OPENAI_BASE_URL" && v.contains("8080")));
        assert!(vars.iter().any(|(k, _)| k == "OPENAI_API_KEY"));
    }

    #[test]
    fn agent_env_vars_claude() {
        let vars = agent_env_vars("claude", 8080, "llama3");
        assert!(vars.iter().any(|(k, v)| k == "ANTHROPIC_BASE_URL" && v.contains("8080")));
    }

    #[test]
    fn recommend_idx_picks_largest_that_fits() {
        let models = vec![
            LocalModel { name: "big".into(), path: PathBuf::from("a"), size_gb: 40.0 },
            LocalModel { name: "mid".into(), path: PathBuf::from("b"), size_gb: 8.0 },
            LocalModel { name: "small".into(), path: PathBuf::from("c"), size_gb: 3.0 },
        ];
        // 16GB RAM → target 9.6GB → first fit is "mid" at index 1
        assert_eq!(recommend_idx(&models, 16.0), 1);
        // 64GB RAM → target 38.4GB → "mid" fits, but "big" at 40GB doesn't → index 1
        assert_eq!(recommend_idx(&models, 64.0), 1);
        // 4GB RAM → target 2.4GB → none fits → last index (2)
        assert_eq!(recommend_idx(&models, 4.0), 2);
    }

    #[test]
    fn available_ram_gb_is_positive() {
        assert!(available_ram_gb() >= 0.0, "RAM detection must return non-negative");
    }
}
