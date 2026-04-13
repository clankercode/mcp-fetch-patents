use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::warn;

use super::browser_search::BROWSER_USER_AGENT;
use super::profile_manager::ProfileManager;

struct PoolEntry {
    browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
}

fn current_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into()))
}

fn pid_alive(pid: u32) -> bool {
    unsafe {
        let ret = libc::kill(pid as i32, 0);
        if ret == 0 {
            return true;
        }
        let errno = *libc::__errno_location();
        errno != libc::ESRCH
    }
}

fn chromium_singleton_target_parts(target: &std::path::Path) -> Option<(String, u32)> {
    let target_name = target.file_name()?.to_string_lossy();
    let (host, pid) = target_name.rsplit_once('-')?;
    let pid = pid.parse::<u32>().ok()?;
    Some((host.to_string(), pid))
}

fn remove_stale_chromium_singleton_lock(profile_dir: &std::path::Path) {
    let singleton_lock = profile_dir.join("SingletonLock");
    let target = match std::fs::read_link(&singleton_lock) {
        Ok(target) => target,
        Err(_) => return,
    };
    let Some((host, pid)) = chromium_singleton_target_parts(&target) else {
        return;
    };
    if host == current_hostname() && !pid_alive(pid) {
        if let Err(e) = std::fs::remove_file(&singleton_lock) {
            warn!("Failed to remove stale Chromium SingletonLock: {}", e);
        }
    }
}

pub struct BrowserPool {
    inner: Arc<Mutex<Option<PoolEntry>>>,
    profiles_dir: Option<PathBuf>,
    profile_name: String,
    headless: bool,
}

impl Clone for BrowserPool {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            profiles_dir: self.profiles_dir.clone(),
            profile_name: self.profile_name.clone(),
            headless: self.headless,
        }
    }
}

impl BrowserPool {
    pub fn new(profiles_dir: Option<PathBuf>, profile_name: String, headless: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            profiles_dir,
            profile_name,
            headless,
        }
    }

    pub async fn get_page(&self) -> Result<chromiumoxide::Page> {
        let mut guard = self.inner.lock().await;

        if let Some(ref entry) = *guard {
            match entry.browser.new_page("about:blank").await {
                Ok(page) => return Ok(page),
                Err(e) => {
                    warn!("Existing browser failed to create page, relaunching: {}", e);
                    entry.handler_task.abort();
                    *guard = None;
                }
            }
        }

        let entry = self.launch_browser().await?;
        match entry.browser.new_page("about:blank").await {
            Ok(page) => {
                *guard = Some(entry);
                Ok(page)
            }
            Err(first_error) => {
                warn!(
                    "Fresh browser failed to create page, relaunching once: {}",
                    first_error
                );
                entry.handler_task.abort();
                let retry_entry = self.launch_browser().await?;
                let page = retry_entry
                    .browser
                    .new_page("about:blank")
                    .await
                    .map_err(|retry_error| {
                        anyhow!(
                            "Failed to create page on fresh browser after retry: {}; first error: {}",
                            retry_error,
                            first_error
                        )
                    })?;
                *guard = Some(retry_entry);
                Ok(page)
            }
        }
    }

    async fn launch_browser(&self) -> Result<PoolEntry> {
        let profile_manager = ProfileManager::new(self.profiles_dir.clone());
        let profile_dir = profile_manager.get_profile_dir(&self.profile_name)?;
        remove_stale_chromium_singleton_lock(&profile_dir);

        let mut config_builder = BrowserConfig::builder()
            .no_sandbox()
            .arg("--disable-gpu")
            .window_size(1280, 900)
            .arg(format!("--user-agent={}", BROWSER_USER_AGENT));

        if !self.headless {
            config_builder = config_builder.with_head();
        }

        config_builder = config_builder.user_data_dir(profile_dir);

        let config = config_builder
            .build()
            .map_err(|e| anyhow!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| anyhow!("Failed to launch browser: {}", e))?;

        let handler_task =
            tokio::spawn(async move { while let Some(_h) = handler.next().await {} });

        tracing::info!(
            "Browser launched successfully for profile {}",
            self.profile_name
        );

        Ok(PoolEntry {
            browser,
            handler_task,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn removes_stale_chromium_singleton_lock_for_dead_local_pid() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join("default");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let lock_path = profile_dir.join("SingletonLock");
        let target = format!("{}-99999999", current_hostname());
        std::os::unix::fs::symlink(target, &lock_path).unwrap();

        remove_stale_chromium_singleton_lock(&profile_dir);

        assert!(!lock_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_chromium_singleton_lock_for_other_host() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join("default");
        std::fs::create_dir_all(&profile_dir).unwrap();
        let lock_path = profile_dir.join("SingletonLock");
        std::os::unix::fs::symlink("other-host-99999999", &lock_path).unwrap();

        remove_stale_chromium_singleton_lock(&profile_dir);

        assert!(std::fs::symlink_metadata(&lock_path).is_ok());
    }
}
