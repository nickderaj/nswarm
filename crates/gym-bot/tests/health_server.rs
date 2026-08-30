//! Network-free Health HTTP boundary helper tests.

use std::time::{Duration, Instant};

use gym_bot::health_server::{RateLimiter, find_header_end};

#[test]
fn header_boundary_and_rate_window_are_exact() {
    assert_eq!(find_header_end(b"a\r\n\r\nb"), Some(1));
    assert_eq!(find_header_end(b"no boundary"), None);

    let limiter = RateLimiter::default();
    let start = Instant::now();
    for _ in 0..30 {
        assert!(limiter.allow(start));
    }
    assert!(!limiter.allow(start));
    assert!(limiter.allow(start + Duration::from_secs(60)));
}
