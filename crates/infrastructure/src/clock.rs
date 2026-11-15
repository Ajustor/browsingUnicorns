use std::time::{SystemTime, UNIX_EPOCH};

use application::ports::Clock;
use domain::Timestamp;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}
