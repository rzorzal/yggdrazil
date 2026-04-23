use std::path::Path;

pub async fn scan_loop(_repo_root: &Path) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
