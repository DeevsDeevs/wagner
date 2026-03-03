use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

pub struct Outbox {
    rate_limit: Duration,
    last_send: Mutex<Instant>,
}

impl Outbox {
    pub fn new(rate_limit_ms: u64) -> Self {
        Self {
            rate_limit: Duration::from_millis(rate_limit_ms),
            last_send: Mutex::new(Instant::now() - Duration::from_secs(10)),
        }
    }

    pub async fn throttle(&self) {
        let mut last = self.last_send.lock().await;
        let elapsed = last.elapsed();
        if elapsed < self.rate_limit {
            tokio::time::sleep(self.rate_limit - elapsed).await;
        }
        *last = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn throttle_does_not_block_first_call() {
        let outbox = Outbox::new(100);
        let start = Instant::now();
        outbox.throttle().await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn throttle_delays_rapid_calls() {
        let outbox = Outbox::new(100);
        outbox.throttle().await;

        // Force last_send to now
        {
            let mut last = outbox.last_send.lock().await;
            *last = Instant::now();
        }

        let start = Instant::now();
        outbox.throttle().await;
        assert!(start.elapsed() >= Duration::from_millis(90));
    }
}
