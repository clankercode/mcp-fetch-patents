use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{ensure, Result};
use chrono::Utc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProfileLock {
    pub pid: u32,
    pub hostname: String,
    pub started_at: String,
    pub purpose: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileBusyError {
    #[error(
        "Profile '{name}' is busy ({purpose}, pid={pid}, host={hostname}, since={started_at})"
    )]
    Busy {
        name: String,
        purpose: String,
        pid: u32,
        hostname: String,
        started_at: String,
    },
}

fn pid_alive(pid: u32) -> bool {
    unsafe {
        let ret = libc::kill(pid as i32, 0);
        if ret == 0 {
            return true;
        }
        let errno = *libc::__errno_location();
        if errno == libc::ESRCH {
            false
        } else {
            true
        }
    }
}

fn current_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into()))
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "Profile name cannot be empty");
    ensure!(
        !name.contains('/') && !name.contains('\\') && !name.contains("..") && !name.contains('\0'),
        "Invalid profile name: {:?}",
        name
    );
    Ok(())
}

pub struct ProfileManager {
    dir: PathBuf,
}

impl ProfileManager {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let dir = dir.unwrap_or_else(|| {
            let base = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("/tmp"))
                        .join(".local")
                        .join("share")
                });
            base.join("patent-search").join("browser-profiles")
        });
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    pub fn profiles_dir(&self) -> &Path {
        &self.dir
    }

    pub fn get_profile_dir(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        let d = self.dir.join(name);
        fs::create_dir_all(&d)?;
        Ok(d)
    }

    pub fn list_profiles(&self) -> Result<Vec<String>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        Ok(names)
    }

    fn lock_path(&self, name: &str) -> PathBuf {
        self.dir.join(name).join(".lock")
    }

    pub fn acquire_lock(&self, name: &str, purpose: &str) -> Result<()> {
        self.get_profile_dir(name)?;
        let lp = self.lock_path(name);

        let lock = ProfileLock {
            pid: std::process::id(),
            hostname: current_hostname(),
            started_at: Utc::now().to_rfc3339(),
            purpose: purpose.to_string(),
        };
        let lock_bytes = serde_json::to_vec(&lock)?;

        match OpenOptions::new().write(true).create_new(true).open(&lp) {
            Ok(mut f) => {
                f.write_all(&lock_bytes)?;
                Ok(())
            }
            Err(_) => {
                let (locked, existing) = self.is_locked(name);
                if locked {
                    if let Some(el) = existing {
                        return Err(ProfileBusyError::Busy {
                            name: name.to_string(),
                            purpose: el.purpose,
                            pid: el.pid,
                            hostname: el.hostname,
                            started_at: el.started_at,
                        }
                        .into());
                    }
                }
                match OpenOptions::new().write(true).create_new(true).open(&lp) {
                    Ok(mut f) => {
                        f.write_all(&lock_bytes)?;
                        Ok(())
                    }
                    Err(_) => {
                        let (_, existing2) = self.is_locked(name);
                        if let Some(el) = existing2 {
                            Err(ProfileBusyError::Busy {
                                name: name.to_string(),
                                purpose: el.purpose,
                                pid: el.pid,
                                hostname: el.hostname,
                                started_at: el.started_at,
                            }
                            .into())
                        } else {
                            Err(ProfileBusyError::Busy {
                                name: name.to_string(),
                                purpose: "unknown".to_string(),
                                pid: 0,
                                hostname: "unknown".to_string(),
                                started_at: String::new(),
                            }
                            .into())
                        }
                    }
                }
            }
        }
    }

    pub fn release_lock(&self, name: &str) -> Result<()> {
        let lp = self.lock_path(name);
        if !lp.exists() {
            return Ok(());
        }
        match fs::read_to_string(&lp) {
            Ok(data) => {
                if let Ok(lock) = serde_json::from_str::<ProfileLock>(&data) {
                    if lock.pid == std::process::id() && lock.hostname == current_hostname() {
                        let _ = fs::remove_file(&lp);
                    }
                }
            }
            Err(_) => {}
        }
        Ok(())
    }

    pub fn is_locked(&self, name: &str) -> (bool, Option<ProfileLock>) {
        let lp = self.lock_path(name);
        if !lp.exists() {
            return (false, None);
        }

        let data = match fs::read_to_string(&lp) {
            Ok(d) => d,
            Err(_) => {
                let _ = fs::remove_file(&lp);
                return (false, None);
            }
        };

        let lock = match serde_json::from_str::<ProfileLock>(&data) {
            Ok(l) => l,
            Err(_) => {
                let _ = fs::remove_file(&lp);
                return (false, None);
            }
        };

        if lock.hostname == current_hostname() && !pid_alive(lock.pid) {
            let _ = fs::remove_file(&lp);
            return (false, None);
        }

        (true, Some(lock))
    }

    pub fn force_release_lock(&self, name: &str) -> Result<()> {
        let lp = self.lock_path(name);
        let _ = fs::remove_file(&lp);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    fn make_mgr() -> (TempDir, ProfileManager) {
        let tmp = TempDir::new().unwrap();
        let mgr = ProfileManager::new(Some(tmp.path().to_path_buf()));
        (tmp, mgr)
    }

    #[test]
    fn acquire_and_release_lock() {
        let (_tmp, mgr) = make_mgr();
        mgr.acquire_lock("default", "search").unwrap();

        let (locked, lock_info) = mgr.is_locked("default");
        assert!(locked);
        assert!(lock_info.is_some());
        let info = lock_info.unwrap();
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.purpose, "search");

        mgr.release_lock("default").unwrap();
        let (locked, _) = mgr.is_locked("default");
        assert!(!locked);
    }

    #[test]
    fn stale_lock_cleaned() {
        let (_tmp, mgr) = make_mgr();
        let profile_dir = mgr.get_profile_dir("test-stale").unwrap();
        let lock_path = profile_dir.join(".lock");

        let stale = ProfileLock {
            pid: 999999988,
            hostname: current_hostname(),
            started_at: Utc::now().to_rfc3339(),
            purpose: "search".to_string(),
        };
        let mut f = File::create(&lock_path).unwrap();
        f.write_all(serde_json::to_vec(&stale).unwrap().as_slice())
            .unwrap();
        drop(f);

        let (locked, _) = mgr.is_locked("test-stale");
        assert!(!locked);
        assert!(!lock_path.exists());
    }

    #[test]
    fn concurrent_lock_fails() {
        let (_tmp, mgr) = make_mgr();
        mgr.acquire_lock("locked-profile", "search").unwrap();

        let err = mgr.acquire_lock("locked-profile", "search-2").unwrap_err();
        let busy = err.downcast::<ProfileBusyError>().unwrap();
        match busy {
            ProfileBusyError::Busy { name, purpose, .. } => {
                assert_eq!(name, "locked-profile");
                assert_eq!(purpose, "search");
            }
        }
    }

    #[test]
    fn name_validation() {
        let (_tmp, mgr) = make_mgr();
        assert!(mgr.get_profile_dir("../../etc/passwd").is_err());
        assert!(mgr.get_profile_dir("..").is_err());
        assert!(mgr.get_profile_dir("foo/bar").is_err());
        assert!(mgr.get_profile_dir("foo\\bar").is_err());
        assert!(mgr.get_profile_dir("ok-name").is_ok());
    }

    #[test]
    fn list_profiles() {
        let (_tmp, mgr) = make_mgr();
        mgr.get_profile_dir("alpha").unwrap();
        mgr.get_profile_dir("beta").unwrap();
        mgr.get_profile_dir("gamma").unwrap();

        let profiles = mgr.list_profiles().unwrap();
        assert_eq!(profiles, vec!["alpha", "beta", "gamma"]);
    }
}
