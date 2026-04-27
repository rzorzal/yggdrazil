use anyhow::{Context, Result};
use dialoguer::{Confirm, Select};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MODEL_INSTALL_DIR: &str = "~/.cache/llama.cpp";
const BIN_INSTALL_DIR: &str = "~/.local/bin";

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
    pub server_bin: Option<PathBuf>,
}

struct RecommendedModel {
    name: &'static str,
    repo: &'static str,
    filename: &'static str,
    approx_size_gb: f64,
    min_ram_gb: f64,
}

const RECOMMENDED_MODELS: &[RecommendedModel] = &[
    RecommendedModel {
        name: "Llama-3.1-8B-Q4_K_M",
        repo: "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
        filename: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        approx_size_gb: 4.92,
        min_ram_gb: 8.0,
    },
    RecommendedModel {
        name: "Llama-3.2-3B-Q4_K_M",
        repo: "bartowski/Llama-3.2-3B-Instruct-GGUF",
        filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        approx_size_gb: 2.02,
        min_ram_gb: 4.0,
    },
];

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
    models.sort_by(|a, b| b.size_gb.partial_cmp(&a.size_gb).unwrap_or(std::cmp::Ordering::Equal));
    models
}

/// Available RAM in GB. Returns 8.0 as safe default if detection fails.
pub fn available_ram_gb() -> f64 {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let bytes = sys.available_memory();
    if bytes == 0 {
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
    // Also check ~/.local/bin directly (may not be in PATH)
    let local_bin = expand_home(BIN_INSTALL_DIR).join("llama-server");
    if local_bin.exists() {
        return Some(local_bin);
    }
    None
}

fn platform_suffix() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x86_64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("ubuntu-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("ubuntu-arm64")
    } else {
        None
    }
}

/// Download a file with inline progress display. Returns bytes written.
fn download_file(url: &str, dest: &Path, label: &str) -> Result<u64> {
    let resp = ureq::get(url)
        .set("User-Agent", "yggdrazil/0.1.5")
        .call()
        .with_context(|| format!("GET {url}"))?;

    let total_bytes: Option<u64> = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok());

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("create {}", dest.display()))?;

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        if let Some(total) = total_bytes {
            let pct = downloaded * 100 / total;
            eprint!(
                "\r  {label}: {:.1} MB / {:.1} MB ({}%)",
                downloaded as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0,
                pct
            );
        } else {
            eprint!("\r  {label}: {:.1} MB", downloaded as f64 / 1_048_576.0);
        }
        let _ = std::io::stderr().flush();
    }
    eprintln!(); // newline after progress
    Ok(downloaded)
}

/// Find the browser_download_url for the platform-specific llama.cpp release asset.
fn llama_release_url() -> Result<String> {
    let suffix = platform_suffix().context(
        "unsupported platform — download llama-server manually from https://github.com/ggerganov/llama.cpp/releases",
    )?;

    let resp = ureq::get("https://api.github.com/repos/ggerganov/llama.cpp/releases/latest")
        .set("User-Agent", "yggdrazil/0.1.5")
        .call()
        .context("fetching llama.cpp release info from GitHub")?;

    let body: serde_json::Value = resp.into_json()?;
    let assets = body["assets"]
        .as_array()
        .context("GitHub response missing assets")?;

    for asset in assets {
        let name = asset["name"].as_str().unwrap_or("");
        if name.contains(suffix) && name.ends_with(".zip") {
            let url = asset["browser_download_url"]
                .as_str()
                .context("missing browser_download_url")?;
            return Ok(url.to_string());
        }
    }

    anyhow::bail!(
        "no llama.cpp release found for platform '{suffix}' — \
         download manually from https://github.com/ggerganov/llama.cpp/releases"
    )
}

fn download_llama_server() -> Result<PathBuf> {
    let install_dir = expand_home(BIN_INSTALL_DIR);
    let dest_bin = install_dir.join("llama-server");

    let url = llama_release_url()?;
    let zip_path = std::env::temp_dir().join("llama-server-release.zip");

    println!("  Downloading llama-server from:");
    println!("  {url}");
    download_file(&url, &zip_path, "llama-server")?;

    std::fs::create_dir_all(&install_dir)?;

    // Extract only llama-server from the zip (use system unzip)
    let out = std::process::Command::new("unzip")
        .args([
            "-jo",                                // junk paths, overwrite
            zip_path.to_str().unwrap(),
            "*/llama-server",
            "-d",
            install_dir.to_str().unwrap(),
        ])
        .output()
        .context("unzip failed — is `unzip` installed?")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("unzip error: {stderr}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest_bin, std::fs::Permissions::from_mode(0o755))?;
    }

    let _ = std::fs::remove_file(&zip_path);
    println!("  Installed: {}", dest_bin.display());

    Ok(dest_bin)
}

/// Ensure llama-server is available. Offers download if missing.
/// Returns Some(path) if available/installed, None if user declined.
pub fn ensure_llama_server() -> Result<Option<PathBuf>> {
    if let Some(bin) = find_llama_server() {
        return Ok(Some(bin));
    }

    println!();
    println!("  llama-server not found in PATH.");
    println!("  ygg can download the latest llama.cpp pre-built binary (~10 MB).");
    println!("  Install location: {}", expand_home(BIN_INSTALL_DIR).display());

    let ok = Confirm::new()
        .with_prompt("Download llama-server now?")
        .default(true)
        .interact()?;

    if !ok {
        return Ok(None);
    }

    let bin = download_llama_server()?;
    Ok(Some(bin))
}

fn pick_recommended_model(ram_gb: f64) -> &'static RecommendedModel {
    // Largest model whose min_ram_gb fits available RAM
    RECOMMENDED_MODELS
        .iter()
        .find(|m| ram_gb >= m.min_ram_gb)
        .unwrap_or(&RECOMMENDED_MODELS[RECOMMENDED_MODELS.len() - 1])
}

fn download_model(rec: &RecommendedModel) -> Result<LocalModel> {
    let dest_dir = expand_home(MODEL_INSTALL_DIR);
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(rec.filename);

    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        rec.repo, rec.filename
    );

    println!("  Downloading model from HuggingFace:");
    println!("  {url}");
    let bytes = download_file(&url, &dest, rec.name)?;
    let size_gb = bytes as f64 / 1_073_741_824.0;

    println!("  Saved: {}", dest.display());
    Ok(LocalModel {
        name: rec.name.to_string(),
        path: dest,
        size_gb,
    })
}

/// Ensure at least one GGUF model is available. Offers download if missing.
pub fn ensure_model(ram_gb: f64) -> Result<LocalModel> {
    let models = find_models();

    if !models.is_empty() {
        return Ok(if models.len() == 1 {
            models.into_iter().next().unwrap()
        } else {
            let rec_idx = recommend_idx(&models, ram_gb);
            let labels: Vec<String> = models
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let tag = if i == rec_idx { " ★" } else { "" };
                    format!("{}{} ({:.1} GB)", m.name, tag, m.size_gb)
                })
                .collect();

            println!("Available RAM: {:.1} GB", ram_gb);
            let idx = Select::new()
                .with_prompt("Select local model")
                .items(&labels)
                .default(rec_idx)
                .interact()?;

            models.into_iter().nth(idx).unwrap()
        });
    }

    // No models found — offer to download
    let rec = pick_recommended_model(ram_gb);
    println!();
    println!("  No GGUF models found.");
    println!(
        "  Recommended for {:.0} GB RAM: {} ({:.1} GB download)",
        ram_gb, rec.name, rec.approx_size_gb
    );
    println!(
        "  Install location: {}",
        expand_home(MODEL_INSTALL_DIR).display()
    );

    let ok = Confirm::new()
        .with_prompt("Download model now?")
        .default(true)
        .interact()?;

    if !ok {
        anyhow::bail!(
            "No model available. Download a GGUF model to {} and retry.",
            expand_home(MODEL_INSTALL_DIR).display()
        );
    }

    download_model(rec)
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
    let ram_gb = available_ram_gb();

    let model = ensure_model(ram_gb)?;
    let server_bin = ensure_llama_server()?;

    let server_port = find_free_port(8080);
    let env_vars = agent_env_vars(agent, server_port, &model.name);

    Ok(LocalSetup {
        model,
        server_port,
        env_vars,
        server_bin,
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

pub fn start_server(
    server_bin: &Path,
    model: &LocalModel,
    port: u16,
) -> Result<std::process::Child> {
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
        assert_eq!(recommend_idx(&models, 16.0), 1);
        assert_eq!(recommend_idx(&models, 4.0), 2);
    }

    #[test]
    fn available_ram_gb_is_non_negative() {
        assert!(available_ram_gb() >= 0.0);
    }

    #[test]
    fn pick_recommended_model_by_ram() {
        assert_eq!(pick_recommended_model(16.0).name, "Llama-3.1-8B-Q4_K_M");
        assert_eq!(pick_recommended_model(3.0).name, "Llama-3.2-3B-Q4_K_M");
    }

    #[test]
    fn platform_suffix_returns_value_on_known_platform() {
        // Will be None only on Windows or unsupported — just assert it doesn't panic
        let _ = platform_suffix();
    }
}
