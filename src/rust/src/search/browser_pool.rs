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
