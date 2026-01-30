use std::time::Instant;

#[derive(Debug, Clone)]
pub struct RateLimiter {
    requests_per_second: f64,
    burst_size: f64,
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(requests_per_second: u32, burst_size: u32) -> Self {
        let rps = requests_per_second as f64;
        let burst = burst_size as f64;
        Self {
            requests_per_second: rps,
            burst_size: burst,
            tokens: burst,
            last_refill: Instant::now(),
        }
    }

    pub fn allow(&mut self) -> Result<(), u64> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.requests_per_second).min(self.burst_size);
            self.last_refill = now;
        }

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            let needed = 1.0 - self.tokens;
            let retry_after = (needed / self.requests_per_second * 1000.0).ceil();
            Err(retry_after as u64)
        }
    }
}
