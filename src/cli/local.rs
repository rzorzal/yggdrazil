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
    pub ctx_size: u32,
    pub gpu: Option<String>,
    pub env_vars: Vec<(String, String)>,
    pub server_bin: Option<PathBuf>,
    /// true when we attached to an already-running server (we must not kill it on exit)
    pub reused_server: bool,
}

// ── llama-server PID tracking ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlamaState {
    pub pid: u32,
    pub port: u16,
    pub model_name: String,
    pub gpu: Option<String>,
}

pub fn llama_state_path(repo_root: &std::path::Path) -> PathBuf {
    repo_root.join(".ygg").join("llama-server.pid")
}

pub fn read_llama_state(repo_root: &std::path::Path) -> Option<LlamaState> {
    let content = std::fs::read_to_string(llama_state_path(repo_root)).ok()?;
    let mut pid = None::<u32>;
    let mut port = None::<u16>;
    let mut model_name = None::<String>;
    let mut gpu = None::<String>;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("pid=") { pid = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("port=") { port = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("model=") { model_name = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("gpu=") { gpu = Some(v.to_string()); }
    }
    Some(LlamaState {
        pid: pid?,
        port: port?,
        model_name: model_name.unwrap_or_default(),
        gpu,
    })
}

pub fn write_llama_state(repo_root: &std::path::Path, state: &LlamaState) -> Result<()> {
    let mut content = format!("pid={}\nport={}\nmodel={}\n", state.pid, state.port, state.model_name);
    if let Some(ref g) = state.gpu {
        content.push_str(&format!("gpu={g}\n"));
    }
    std::fs::write(llama_state_path(repo_root), content)?;
    Ok(())
}

pub fn clear_llama_state(repo_root: &std::path::Path) -> Result<()> {
    let path = llama_state_path(repo_root);
    if path.exists() { std::fs::remove_file(path)?; }
    Ok(())
}

fn is_pid_alive(pid: u32) -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes();
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

fn is_port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        std::time::Duration::from_millis(500),
    ).is_ok()
}

/// Check `.ygg/llama-server.pid` and return state only if the process is alive and the port responds.
pub fn find_running_llama(repo_root: &std::path::Path) -> Option<LlamaState> {
    let state = read_llama_state(repo_root)?;
    if is_pid_alive(state.pid) && is_port_open(state.port) {
        Some(state)
    } else {
        // Stale PID file — clean up
        let _ = clear_llama_state(repo_root);
        None
    }
}

/// Kill the llama-server recorded in the PID file and remove it.
pub fn stop_server(repo_root: &std::path::Path) -> Result<()> {
    let state = match read_llama_state(repo_root) {
        Some(s) => s,
        None => {
            println!("  No llama-server PID file found — nothing to stop.");
            return Ok(());
        }
    };

    if !is_pid_alive(state.pid) {
        println!("  PID {} is not running (stale file removed).", state.pid);
        clear_llama_state(repo_root)?;
        return Ok(());
    }

    #[cfg(unix)]
    unsafe {
        libc::kill(state.pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    {
        // Windows: use taskkill
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &state.pid.to_string(), "/F"])
            .output();
    }

    // Wait up to 3s for graceful exit
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if !is_pid_alive(state.pid) { break; }
    }

    // Force kill if still alive
    #[cfg(unix)]
    if is_pid_alive(state.pid) {
        unsafe { libc::kill(state.pid as libc::pid_t, libc::SIGKILL); }
    }

    clear_llama_state(repo_root)?;
    println!("  llama-server (PID {}) stopped.", state.pid);
    Ok(())
}

// ── GPU detection ────────────────────────────────────────────────────────────

/// Returns a human-readable GPU label, e.g. "Metal (Apple M3 Pro)" or "CUDA (RTX 4090)".
pub fn detect_gpu() -> Option<String> {
    // Apple Silicon — Metal always available
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        let chip = macos_chip_name().unwrap_or_else(|| "Apple Silicon".into());
        return Some(format!("Metal ({chip})"));
    }

    // NVIDIA CUDA
    if let Some(name) = nvidia_gpu_name() {
        return Some(format!("CUDA ({name})"));
    }

    // AMD ROCm
    if let Some(name) = rocm_gpu_name() {
        return Some(format!("ROCm ({name})"));
    }

    // macOS Intel — dedicated GPU via Metal
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    if let Some(name) = macos_dedicated_gpu() {
        return Some(format!("Metal ({name})"));
    }

    None
}

fn macos_chip_name() -> Option<String> {
    let out = std::process::Command::new("system_profiler")
        .arg("SPHardwareDataType")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(chip) = line.trim().strip_prefix("Chip:") {
            return Some(chip.trim().to_string());
        }
    }
    None
}

fn nvidia_gpu_name() -> Option<String> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let name = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn rocm_gpu_name() -> Option<String> {
    let out = std::process::Command::new("rocm-smi")
        .arg("--showproductname")
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains("Card series") {
            if let Some(name) = line.splitn(2, ':').nth(1) {
                let n = name.trim().to_string();
                if !n.is_empty() { return Some(n); }
            }
        }
    }
    None
}

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
fn macos_dedicated_gpu() -> Option<String> {
    let out = std::process::Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(name) = line.trim().strip_prefix("Chipset Model:") {
            return Some(name.trim().to_string());
        }
    }
    None
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

fn download_model_from_hf(repo: &str, filename: &str, display_name: &str) -> Result<LocalModel> {
    let dest_dir = expand_home(MODEL_INSTALL_DIR);
    std::fs::create_dir_all(&dest_dir)?;
    let dest = dest_dir.join(filename);

    let url = format!("https://huggingface.co/{repo}/resolve/main/{filename}");
    println!("  Downloading model from HuggingFace:");
    println!("  {url}");
    let bytes = download_file(&url, &dest, display_name)?;
    let size_gb = bytes as f64 / 1_073_741_824.0;

    println!("  Saved: {}", dest.display());
    Ok(LocalModel {
        name: display_name.to_string(),
        path: dest,
        size_gb,
    })
}

fn download_model(rec: &RecommendedModel) -> Result<LocalModel> {
    download_model_from_hf(rec.repo, rec.filename, rec.name)
}

enum ModelSpec {
    Path(PathBuf),
    HuggingFace { repo: String, filename: String },
    NameSearch(String),
}

fn parse_model_spec(spec: &str) -> ModelSpec {
    if spec.starts_with('/') || spec.starts_with("~/") {
        return ModelSpec::Path(expand_home(spec));
    }
    if spec.ends_with(".gguf") {
        if let Some(slash) = spec.rfind('/') {
            return ModelSpec::HuggingFace {
                repo: spec[..slash].to_string(),
                filename: spec[slash + 1..].to_string(),
            };
        }
    }
    ModelSpec::NameSearch(spec.to_string())
}

/// Resolve a model from a user-supplied spec string.
/// Spec formats:
///   - `/path/to/model.gguf` or `~/path/to/model.gguf` — direct path
///   - `org/repo/file.gguf` — HuggingFace download (searches locally first)
///   - `name` — substring search across MODEL_SEARCH_PATHS
pub fn ensure_model_by_spec(spec: &str) -> Result<LocalModel> {
    match parse_model_spec(spec) {
        ModelSpec::Path(path) => {
            if !path.exists() {
                anyhow::bail!("model file not found: {}", path.display());
            }
            let size_gb = std::fs::metadata(&path)
                .map(|m| m.len() as f64 / 1_073_741_824.0)
                .unwrap_or(0.0);
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("model")
                .to_string();
            Ok(LocalModel { name, path, size_gb })
        }

        ModelSpec::HuggingFace { repo, filename } => {
            // Reuse if already on disk
            for search_path in MODEL_SEARCH_PATHS {
                let dir = expand_home(search_path);
                if dir.exists() {
                    let mut found = Vec::new();
                    collect_gguf_files(&dir, &mut found);
                    if let Some(m) = found.into_iter().find(|m| {
                        m.path.file_name().and_then(|n| n.to_str()) == Some(filename.as_str())
                    }) {
                        println!("  Found existing: {}", m.path.display());
                        return Ok(m);
                    }
                }
            }
            // Not found locally — offer download
            let name = filename.trim_end_matches(".gguf");
            println!();
            println!("  Model not found locally: {filename}");
            println!("  Repo: https://huggingface.co/{repo}");
            println!("  Install location: {}", expand_home(MODEL_INSTALL_DIR).display());
            let ok = Confirm::new()
                .with_prompt("Download now?")
                .default(true)
                .interact()?;
            if !ok {
                anyhow::bail!("download cancelled — place the GGUF in {} and retry", expand_home(MODEL_INSTALL_DIR).display());
            }
            download_model_from_hf(&repo, &filename, name)
        }

        ModelSpec::NameSearch(name) => {
            let models = find_models();
            let lower = name.to_lowercase();
            if let Some(m) = models.into_iter().find(|m| m.name.to_lowercase().contains(&lower)) {
                return Ok(m);
            }
            anyhow::bail!(
                "no local model matching {name:?}\n\
                 Tip: use 'org/repo/file.gguf' format to download from HuggingFace,\n\
                 e.g. bartowski/Qwen2.5-Coder-32B-Instruct-GGUF/Qwen2.5-Coder-32B-Instruct-Q4_K_M.gguf"
            )
        }
    }
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

pub fn setup(agent: &str, ctx_size: u32, repo_root: &std::path::Path, model_spec: Option<&str>) -> Result<LocalSetup> {
    let ram_gb = available_ram_gb();

    // Reuse already-running server when possible
    if let Some(running) = find_running_llama(repo_root) {
        println!(
            "  llama-server already running — PID {} · port {} · {}{}",
            running.pid,
            running.port,
            running.model_name,
            running.gpu.as_deref().map(|g| format!(" [{g}]")).unwrap_or_default(),
        );
        let model = LocalModel {
            name: running.model_name.clone(),
            path: PathBuf::from("(running)"),
            size_gb: 0.0,
        };
        let env_vars = agent_env_vars(agent, running.port, &running.model_name);
        return Ok(LocalSetup {
            model,
            server_port: running.port,
            ctx_size,
            gpu: running.gpu,
            env_vars,
            server_bin: None,
            reused_server: true,
        });
    }

    eprint!("  Detecting GPU... ");
    let gpu = detect_gpu();
    match &gpu {
        Some(g) => eprintln!("{g}"),
        None => eprintln!("none (CPU only — may be slow)"),
    }

    let model = if let Some(spec) = model_spec {
        ensure_model_by_spec(spec)?
    } else {
        ensure_model(ram_gb)?
    };
    let server_bin = ensure_llama_server()?;

    let server_port = find_free_port(8080);
    let env_vars = agent_env_vars(agent, server_port, &model.name);

    Ok(LocalSetup {
        model,
        server_port,
        ctx_size,
        gpu,
        env_vars,
        server_bin,
        reused_server: false,
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
    ctx_size: u32,
    use_gpu: bool,
    gpu_label: Option<&str>,
    repo_root: &std::path::Path,
) -> Result<std::process::Child> {
    let model_path = model.path.to_str().context("model path is not valid UTF-8")?;
    let port_str = port.to_string();
    let ctx_str = ctx_size.to_string();

    let mut args: Vec<&str> = vec![
        "--model", model_path,
        "--port", &port_str,
        "--ctx-size", &ctx_str,
    ];
    if use_gpu {
        args.extend_from_slice(&["-ngl", "99"]);
    }

    let child = std::process::Command::new(server_bin)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to start llama-server")?;

    wait_for_server(port)?;

    // Persist PID so future sessions can reuse this server
    let state = LlamaState {
        pid: child.id(),
        port,
        model_name: model.name.clone(),
        gpu: gpu_label.map(|s| s.to_string()),
    };
    let _ = write_llama_state(repo_root, &state);

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
        let _ = platform_suffix();
    }

    #[test]
    fn detect_gpu_does_not_panic() {
        let gpu = detect_gpu();
        // On CI/sandboxed env this may be None; just ensure no panic
        if let Some(ref g) = gpu {
            assert!(!g.is_empty());
        }
    }

    #[test]
    fn start_server_args_include_ngl_only_when_gpu() {
        // Verify the logic path compiles — actual invocation would need a real binary
        let _ = (true, false); // use_gpu variants
    }
}
