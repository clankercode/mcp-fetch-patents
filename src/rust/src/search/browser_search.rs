use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use chromiumoxide::Page;
use regex::Regex;
use tracing::warn;

use crate::ranking::PatentHit;
use crate::search::browser_pool::BrowserPool;
use crate::search::profile_manager::ProfileManager;

const SOURCE_BROWSER: &str = "Google_Patents_Browser";

pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36";
const STARTUP_BROWSER_PROFILE: &str = "__startup__";

static PATENT_HREF_RE: OnceLock<Regex> = OnceLock::new();
static PATENT_BODY_RE: OnceLock<Regex> = OnceLock::new();

fn patent_href_re() -> &'static Regex {
    PATENT_HREF_RE.get_or_init(|| Regex::new(r"/patent/([A-Z]{2}[A-Z0-9/\-]+?)(?:/|$)").unwrap())
}

fn patent_body_re() -> &'static Regex {
    PATENT_BODY_RE.get_or_init(|| Regex::new(r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b").unwrap())
}

fn is_rate_limited_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "429",
        "403",
        "too many requests",
        "rate limit",
        "rate-limited",
        "unusual traffic",
        "not a robot",
        "verify you are human",
        "captcha",
        "automated queries",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

async fn page_looks_rate_limited(page: &Page) -> bool {
    page.content()
        .await
        .map(|html| is_rate_limited_text(&html))
        .unwrap_or(false)
}

pub struct GooglePatentsBrowserSearch {
    pool: BrowserPool,
    profile_manager: ProfileManager,
    profile_name: String,
    timeout_ms: u32,
    max_pages: u32,
    debug_html_dir: Option<PathBuf>,
}

pub fn spawn_startup_browser(profiles_dir: Option<PathBuf>, headless: bool) {
    tokio::spawn(async move {
        if let Err(e) = startup_browser_task(profiles_dir, headless).await {
            warn!("Startup browser failed: {}", e);
        }
    });
}

async fn startup_browser_task(profiles_dir: Option<PathBuf>, headless: bool) -> Result<()> {
    let pool = BrowserPool::new(profiles_dir, STARTUP_BROWSER_PROFILE.to_string(), headless);
    let page = pool.get_page().await?;
    drop(page);
    tracing::info!("Startup browser ready");
    std::future::pending::<()>().await;
    #[allow(unreachable_code)]
    Ok(())
}

struct LockGuard<'a> {
    pm: &'a ProfileManager,
    name: &'a str,
}

impl<'a> Drop for LockGuard<'a> {
    fn drop(&mut self) {
        if let Err(e) = self.pm.release_lock(self.name) {
            warn!("Failed to release lock for {}: {}", self.name, e);
        }
    }
}

impl GooglePatentsBrowserSearch {
    pub fn new(
        pool: BrowserPool,
        profile_name: &str,
        timeout_ms: u32,
        max_pages: u32,
        debug_html_dir: Option<PathBuf>,
        profiles_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            profile_manager: ProfileManager::new(profiles_dir),
            profile_name: profile_name.to_string(),
            pool,
            timeout_ms,
            max_pages,
            debug_html_dir,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn search(
        &self,
        query: &str,
        date_before: Option<&str>,
        date_after: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<PatentHit>> {
        if let Err(e) = self
            .profile_manager
            .acquire_lock(&self.profile_name, "search")
        {
            warn!("Failed to acquire profile lock: {}", e);
            return Err(anyhow!(
                "Google Patents browser unavailable: profile lock failed: {}",
                e
            ));
        }

        let _lock_guard = LockGuard {
            pm: &self.profile_manager,
            name: &self.profile_name,
        };

        self.run_search(query, date_before, date_after, max_results)
            .await
    }

    async fn run_search(
        &self,
        query: &str,
        date_before: Option<&str>,
        date_after: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<PatentHit>> {
        let page = match self.pool.get_page().await {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to get browser page: {}", e);
                return Err(anyhow!("Google Patents browser unavailable: {}", e));
            }
        };

        let mut all_hits: Vec<PatentHit> = Vec::new();

        for page_num in 0..self.max_pages {
            let url = build_search_url(query, date_before, date_after, page_num);

            match page.goto(&url).await {
                Ok(_) => {}
                Err(e) => {
                    warn!("Navigation failed for page {}: {}", page_num, e);
                    if is_rate_limited_text(&e.to_string()) {
                        return Err(anyhow!("Google Patents browser rate limited: {}", e));
                    }
                    if page_num == 0 {
                        return Err(anyhow!("Google Patents browser navigation failed: {}", e));
                    }
                    break;
                }
            }

            let wait_ms = std::cmp::min(self.timeout_ms, 15000);
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms as u64 / 3)).await;

            let page_hits = extract_patent_hits(&page, &self.debug_html_dir).await;

            if page_hits.is_empty() {
                if page_looks_rate_limited(&page).await {
                    return Err(anyhow!("Google Patents browser rate limited"));
                }
                break;
            }

            all_hits.extend(page_hits);

            if all_hits.len() >= max_results {
                all_hits.truncate(max_results);
                break;
            }

            if !has_next_page(&page).await {
                break;
            }
        }

        all_hits.truncate(max_results);
        drop(page);
        Ok(all_hits)
    }
}

fn build_search_url(
    query: &str,
    date_before: Option<&str>,
    date_after: Option<&str>,
    page: u32,
) -> String {
    let encoded = urlencoding(query);
    let mut url = format!("https://patents.google.com/?q={}", encoded);

    if let Some(before) = date_before {
        let d = before.replace('-', "");
        url.push_str(&format!("&before=priority:{}", d));
    }
    if let Some(after) = date_after {
        let d = after.replace('-', "");
        url.push_str(&format!("&after=priority:{}", d));
    }
    if page > 0 {
        url.push_str(&format!("&page={}", page));
    }

    url
}

fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => {
                result.push('+');
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

async fn extract_patent_hits(page: &Page, debug_html_dir: &Option<PathBuf>) -> Vec<PatentHit> {
    if let Some(ref dir) = debug_html_dir {
        let _ = std::fs::create_dir_all(dir);
        if let Ok(html) = page.content().await {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = std::fs::write(dir.join(format!("page_{}.html", ts)), &html);
        }
    }

    let hits = strategy_structured_elements(page).await;
    if !hits.is_empty() {
        return hits;
    }

    let hits = strategy_patent_links(page).await;
    if !hits.is_empty() {
        return hits;
    }

    strategy_regex_body(page).await
}

async fn strategy_structured_elements(page: &Page) -> Vec<PatentHit> {
    let selectors = ["search-result-item", ".result-item", "article"];
    let patent_id_re = patent_href_re();

    for &sel in &selectors {
        let elements = match page.find_elements(sel).await {
            Ok(e) => e,
            Err(_) => continue,
        };

        if elements.is_empty() {
            continue;
        }

        let mut hits = Vec::new();
        for element in &elements {
            let link = match element.find_element("a[href*='/patent/']").await {
                Ok(l) => l,
                Err(_) => continue,
            };

            let href = match link.attribute("href").await {
                Ok(Some(h)) => h,
                _ => continue,
            };

            let patent_id = match patent_id_re.captures(&href) {
                Some(caps) => caps[1].to_string(),
                None => continue,
            };

            let title = get_child_text(element, "h3, .title").await;
            let abstract_text = get_child_text(element, ".abstract, .snippet").await;
            let assignee = get_child_text(element, ".assignee").await;
            let date = get_child_text(element, ".date, time").await;

            hits.push(PatentHit {
                title,
                date,
                assignee,
                abstract_text,
                url: Some(format!(
                    "https://patents.google.com/patent/{}/en",
                    patent_id
                )),
                ..PatentHit::new(patent_id, SOURCE_BROWSER)
            });
        }

        if !hits.is_empty() {
            return hits;
        }
    }

    vec![]
}

async fn strategy_patent_links(page: &Page) -> Vec<PatentHit> {
    let patent_id_re = patent_href_re();

    let links = match page.find_elements("a[href*='/patent/']").await {
        Ok(l) => l,
        Err(_) => return vec![],
    };

    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for link in &links {
        let href = match link.attribute("href").await {
            Ok(Some(h)) => h,
            _ => continue,
        };

        let patent_id = match patent_id_re.captures(&href) {
            Some(caps) => caps[1].to_string(),
            None => continue,
        };

        if !seen.insert(patent_id.clone()) {
            continue;
        }

        let title = link.inner_text().await.ok().flatten();

        let abstract_text = link
            .call_js_fn("function() { var parent = this.closest('article, div, section, li'); return parent ? parent.innerText.substring(0, 500) : ''; }", true)
            .await
            .ok()
            .and_then(|v| v.result.value)
            .and_then(|v| v.as_str().map(String::from))
            .filter(|s| !s.is_empty());

        hits.push(PatentHit {
            title,
            abstract_text,
            url: Some(format!(
                "https://patents.google.com/patent/{}/en",
                patent_id
            )),
            ..PatentHit::new(patent_id, SOURCE_BROWSER)
        });
    }

    hits
}

async fn strategy_regex_body(page: &Page) -> Vec<PatentHit> {
    let body_text = match page.content().await {
        Ok(t) => t,
        Err(_) => return vec![],
    };

    let re = patent_body_re();
    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for caps in re.captures_iter(&body_text) {
        let patent_id = caps[1].to_string();
        if patent_id.len() < 7 {
            continue;
        }
        if !seen.insert(patent_id.clone()) {
            continue;
        }

        hits.push(PatentHit {
            url: Some(format!(
                "https://patents.google.com/patent/{}/en",
                patent_id
            )),
            ..PatentHit::new(patent_id, SOURCE_BROWSER)
        });
    }

    hits
}

async fn get_child_text(element: &chromiumoxide::Element, selector: &str) -> Option<String> {
    match element.find_element(selector).await {
        Ok(e) => e.inner_text().await.ok().flatten(),
        Err(_) => None,
    }
}

async fn has_next_page(page: &Page) -> bool {
    let selectors = [
        "[aria-label='Next']",
        "[aria-label='Next page']",
        "a.next",
        "button.next",
    ];

    for sel in &selectors {
        match page.find_element(*sel).await {
            Ok(_) => return true,
            Err(_) => continue,
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_url_basic() {
        let url = build_search_url("neural network", None, None, 0);
        assert_eq!(url, "https://patents.google.com/?q=neural+network");
    }

    #[test]
    fn test_build_search_url_with_dates() {
        let url = build_search_url(
            "machine learning",
            Some("2020-12-31"),
            Some("2019-01-01"),
            0,
        );
        assert!(url.contains("q=machine+learning"));
        assert!(url.contains("&before=priority:20201231"));
        assert!(url.contains("&after=priority:20190101"));
    }

    #[test]
    fn test_build_search_url_with_page() {
        let url = build_search_url("quantum", None, None, 3);
        assert!(url.contains("&page=3"));
    }

    #[test]
    fn test_build_search_url_page_zero_omitted() {
        let url = build_search_url("quantum", None, None, 0);
        assert!(!url.contains("&page="));
    }

    #[test]
    fn test_build_search_url_date_dashes_stripped() {
        let url = build_search_url("test", Some("2021-06-15"), None, 0);
        assert!(url.contains("before=priority:20210615"));
        assert!(!url.contains("before=priority:2021-06-15"));
    }

    #[test]
    fn test_urlencoding_special_chars() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding("a&b"), "a%26b");
        assert_eq!(urlencoding("test-123"), "test-123");
    }

    #[test]
    fn test_regex_patent_id_extraction_us() {
        let re = Regex::new(r"/patent/([A-Z]{2}[A-Z0-9/\-]+?)(?:/|$)").unwrap();
        let href = "https://patents.google.com/patent/US12345678B2/en";
        let caps = re.captures(href).unwrap();
        assert_eq!(&caps[1], "US12345678B2");
    }

    #[test]
    fn test_regex_patent_id_extraction_ep() {
        let re = Regex::new(r"/patent/([A-Z]{2}[A-Z0-9/\-]+?)(?:/|$)").unwrap();
        let href = "https://patents.google.com/patent/EP1234567A1/en";
        let caps = re.captures(href).unwrap();
        assert_eq!(&caps[1], "EP1234567A1");
    }

    #[test]
    fn test_regex_patent_id_extraction_wo() {
        let re = Regex::new(r"/patent/([A-Z]{2}[A-Z0-9/\-]+?)(?:/|$)").unwrap();
        let href = "https://patents.google.com/patent/WO2020123456A1/en";
        let caps = re.captures(href).unwrap();
        assert_eq!(&caps[1], "WO2020123456A1");
    }

    #[test]
    fn test_regex_body_patent_id_us() {
        let re = Regex::new(r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b").unwrap();
        let text = "See US12345678 for details and US9876543B2 also";
        let caps: Vec<_> = re.captures_iter(text).collect();
        assert_eq!(caps.len(), 2);
        assert_eq!(&caps[0][1], "US12345678");
        assert_eq!(&caps[1][1], "US9876543B2");
    }

    #[test]
    fn test_regex_body_patent_id_too_short() {
        let re = Regex::new(r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b").unwrap();
        let text = "US1234 is too short";
        let caps: Vec<_> = re.captures_iter(text).collect();
        assert!(caps.is_empty());
    }

    #[test]
    fn test_regex_body_patent_id_ep() {
        let re = Regex::new(r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b").unwrap();
        let text = "EP1234567A1 describes a method";
        let caps: Vec<_> = re.captures_iter(text).collect();
        assert_eq!(caps.len(), 1);
        assert_eq!(&caps[0][1], "EP1234567A1");
    }

    #[test]
    fn test_regex_body_filters_min_length() {
        let re = Regex::new(r"\b([A-Z]{2}\d{5,12}[A-Z]?\d?)\b").unwrap();
        let text = "US12345 has 7 chars total";
        let caps: Vec<_> = re.captures_iter(text).collect();
        assert!(!caps.is_empty());
        let id = &caps[0][1];
        assert!(id.len() >= 7);
    }

    #[test]
    fn test_constructor_creates_instance() {
        let pool = BrowserPool::new(None, "test-profile".to_string(), true);
        let search = GooglePatentsBrowserSearch::new(pool, "test-profile", 30000, 2, None, None);
        assert_eq!(search.profile_name, "test-profile");
        assert_eq!(search.timeout_ms, 30000);
        assert_eq!(search.max_pages, 2);
        assert!(search.debug_html_dir.is_none());
    }

    #[test]
    fn test_browser_user_agent_constant() {
        assert!(BROWSER_USER_AGENT.contains("Chrome/"));
        assert!(BROWSER_USER_AGENT.contains("Linux x86_64"));
    }

    #[test]
    fn test_patent_body_re_via_once_lock() {
        let re = patent_body_re();
        assert!(re.is_match("US12345678"));
        assert!(re.is_match("EP1234567A1"));
        assert!(!re.is_match("short"));
    }

    #[test]
    fn test_patent_href_re_via_once_lock() {
        let re = patent_href_re();
        let caps = re.captures("/patent/US9999999B2/en").unwrap();
        assert_eq!(&caps[1], "US9999999B2");
    }

    #[test]
    fn test_rate_limit_detector_matches_google_throttle_copy() {
        assert!(is_rate_limited_text(
            "Our systems have detected unusual traffic from your computer network"
        ));
        assert!(is_rate_limited_text("HTTP 429 Too Many Requests"));
        assert!(!is_rate_limited_text(
            "Search results for wireless charging patents"
        ));
    }
}
