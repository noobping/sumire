use std::time::{Duration, SystemTime};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub(crate) fn parse_rfc3339_system_time(s: &str) -> Option<SystemTime> {
    let odt = OffsetDateTime::parse(s, &Rfc3339).ok()?;
    let unix = odt.unix_timestamp(); // seconds
    let nanos = odt.nanosecond(); // 0..1e9

    let t = if unix >= 0 {
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(unix as u64))?
            .checked_add(Duration::from_nanos(nanos as u64))?
    } else {
        SystemTime::UNIX_EPOCH
            .checked_sub(Duration::from_secs((-unix) as u64))?
            .checked_add(Duration::from_nanos(nanos as u64))?
    };

    Some(t)
}
