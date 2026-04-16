use std::sync::Arc;
use std::time::Duration;

pub struct RateLimiter {
    sem: Arc<tokio::sync::Semaphore>,
    hold_time: Duration,
}

impl RateLimiter {
    pub fn new(max_concurrent: usize, hold_time_secs: u64) -> Self {
        RateLimiter {
            sem: Arc::new(tokio::sync::Semaphore::new(max_concurrent)),
            hold_time: Duration::from_secs(hold_time_secs),
        }
    }

    pub fn sem(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.sem
    }

    pub fn hold_time(&self) -> Duration {
        self.hold_time
    }

    pub async fn acquire(&self) {
        let permit = self.sem.acquire().await.expect("semaphore closed");
        tokio::time::sleep(self.hold_time).await;
        drop(permit);
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        RateLimiter {
            sem: Arc::clone(&self.sem),
            hold_time: self.hold_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_holds_for_hold_time() {
        let limiter = RateLimiter::new(1, 1);
        let start = std::time::Instant::now();
        limiter.acquire().await;
        let elapsed = start.elapsed().as_secs();
        assert_eq!(elapsed, 1);
    }

    #[tokio::test]
    async fn rate_limiter_allows_one_at_a_time() {
        let limiter = RateLimiter::new(1, 1);
        let start = std::time::Instant::now();

        let limiter_clone = limiter.clone();
        let handle = tokio::spawn(async move {
            let permit = limiter_clone.sem.acquire_owned().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
            drop(permit);
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        let permit2 = limiter.sem.acquire_owned().await;
        let elapsed = start.elapsed().as_secs_f64();
        drop(permit2);
        handle.await.unwrap();

        assert!(elapsed >= 1.0, "should wait for hold_time before second acquire, got {}", elapsed);
    }
}
