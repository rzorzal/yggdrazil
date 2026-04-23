use anyhow::{Context, Result};

const GITHUB_REPO: &str = "rzorzal/yggdrazil";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn detect_target() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Some("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return Some("aarch64-unknown-linux-gnu");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Some("x86_64-apple-darwin");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some("aarch64-apple-darwin");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some("x86_64-pc-windows-msvc");
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    return None;
}

fn is_newer(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Option<(u32, u32, u32)> {
        let v = v.trim_start_matches('v');
        let mut p = v.splitn(3, '.');
        Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?))
    }
    match (parse(current), parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

pub fn run() -> Result<()> {
    println!("Current version: v{CURRENT_VERSION}");
    println!("Checking for updates...");

    let api_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body: serde_json::Value = ureq::get(&api_url)
        .set("User-Agent", &format!("ygg/{CURRENT_VERSION}"))
        .call()
        .context("failed to reach GitHub API")?
        .into_json()
        .context("failed to parse GitHub API response")?;

    let tag = body["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing tag_name in GitHub API response"))?;
    let latest = tag.trim_start_matches('v');

    if !is_newer(CURRENT_VERSION, latest) {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    println!("Update available: v{CURRENT_VERSION} → v{latest}");

    let target = detect_target().ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported platform — download manually from https://github.com/{GITHUB_REPO}/releases"
        )
    })?;

    #[cfg(windows)]
    {
        println!(
            "Windows auto-update not supported. Download from:\nhttps://github.com/{GITHUB_REPO}/releases/tag/{tag}"
        );
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let filename = format!("ygg-{tag}-{target}.tar.gz");
        let url = format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{filename}");

        if !dialoguer::Confirm::new()
            .with_prompt(format!("Download and install v{latest}?"))
            .default(true)
            .interact()?
        {
            println!("Update cancelled.");
            return Ok(());
        }

        println!("Downloading {filename}...");

        let tmp_dir = tempfile::tempdir().context("failed to create temp dir")?;
        let archive_path = tmp_dir.path().join(&filename);

        let response = ureq::get(&url)
            .set("User-Agent", &format!("ygg/{CURRENT_VERSION}"))
            .call()
            .context("download failed")?;

        let mut reader = response.into_reader();
        let mut file = std::fs::File::create(&archive_path).context("failed to create temp file")?;
        std::io::copy(&mut reader, &mut file).context("failed to write download")?;

        println!("Extracting...");

        let status = std::process::Command::new("tar")
            .args(["xzf", archive_path.to_str().unwrap(), "-C", tmp_dir.path().to_str().unwrap(), "ygg"])
            .status()
            .context("tar extraction failed")?;

        if !status.success() {
            anyhow::bail!("tar extraction failed with status {status}");
        }

        let new_binary = tmp_dir.path().join("ygg");
        let current_exe = std::env::current_exe().context("failed to locate current binary")?;

        // Set executable bit before replacing
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&new_binary)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&new_binary, perms)?;
        }

        std::fs::rename(&new_binary, &current_exe)
            .context("failed to replace binary — try running with sudo")?;

        println!("✓ Updated to v{latest}. Run `ygg --version` to confirm.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_detects_upgrade() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
    }

    #[test]
    fn is_newer_strips_v_prefix() {
        assert!(is_newer("0.1.0", "v0.2.0"));
    }

    #[test]
    fn detect_target_returns_some_on_known_platform() {
        // This test runs on a known CI platform, so should be Some
        assert!(detect_target().is_some());
    }
}
