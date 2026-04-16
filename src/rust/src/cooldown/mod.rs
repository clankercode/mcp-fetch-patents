use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct CooldownState {
    pub retry_count: u8,
    pub next_available_at: Instant,
}

pub struct SourceCooldown {
    inner: Mutex<HashMap<String, CooldownState>>,
}

impl SourceCooldown {
    pub fn new() -> Self {
        SourceCooldown {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_cool(&self, source: &str) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match inner.get(source) {
            None => true,
            Some(state) => Instant::now() >= state.next_available_at,
        }
    }

    pub fn mark_rate_limited(&self, source: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let state = inner.entry(source.to_string()).or_insert(CooldownState {
            retry_count: 0,
            next_available_at: Instant::now(),
        });
        if state.retry_count >= 4 {
            return;
        }
        state.retry_count += 1;
        let minutes = 1 << (state.retry_count - 1);
        state.next_available_at = Instant::now() + std::time::Duration::from_secs(minutes * 60);
    }

    pub fn wait_duration(&self, source: &str) -> Option<std::time::Duration> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.get(source).map(|state| {
            let now = Instant::now();
            if now >= state.next_available_at {
                std::time::Duration::ZERO
            } else {
                state.next_available_at - now
            }
        })
    }

    pub fn is_exhausted(&self, source: &str) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .get(source)
            .map(|s| s.retry_count >= 4)
            .unwrap_or(false)
    }
}

impl Default for SourceCooldown {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_source_is_cool() {
        let cooldown = SourceCooldown::new();
        assert!(cooldown.is_cool("USPTO"));
        assert!(cooldown.is_cool("EPO_OPS"));
    }

    #[test]
    fn exhausted_source_is_not_cool() {
        let cooldown = SourceCooldown::new();
        for _ in 0..4 {
            cooldown.mark_rate_limited("USPTO");
        }
        assert!(!cooldown.is_cool("USPTO"));
        assert!(cooldown.is_exhausted("USPTO"));
    }

    #[test]
    fn cooldown_blocks_until_timer_expires() {
        use std::time::Duration;
        let cooldown = SourceCooldown::new();
        cooldown.mark_rate_limited("USPTO");
        assert!(!cooldown.is_cool("USPTO"));
        let wait = cooldown.wait_duration("USPTO").unwrap();
        assert!(wait > Duration::ZERO);
        assert!(wait <= Duration::from_secs(60));
    }
}
