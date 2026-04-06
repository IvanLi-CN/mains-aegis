#[cfg(any(test, codex_host_test))]
pub use std::time::{Duration, Instant};

#[cfg(not(any(test, codex_host_test)))]
pub use esp_hal::time::{Duration, Instant};
