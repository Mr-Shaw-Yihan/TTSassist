// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! `Sec-MS-GEC` DRM token generation and Edge-protocol time helpers.
//!
//! The Edge Read-Aloud endpoint requires a `Sec-MS-GEC` query parameter whose
//! value is a SHA-256 hash derived from the current Windows file-time and a
//! fixed client token.  The token is valid inside a rolling 5-minute window;
//! when the server's clock and ours disagree by more than that, the handshake
//! returns HTTP 403 and the connection layer retries with an adjusted skew.

use rand::RngCore;
use std::time::{SystemTime, UNIX_EPOCH};

/// Long-stable trusted-client token embedded in the Edge extension.
/// (Source: `edge-tts` / `rany2` - unchanged for years.)
pub(crate) const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";

/// Edge stable-channel version string.  Microsoft rotates this on release
/// cycles; when every synthesis starts returning HTTP 403, re-pin this to the
/// current Edge version and republish the crate.
/// 2026-10：令牌升级 v2 后版本串前缀由 1- 改为 2-（主版本号 143 仍被服务端接受，
/// 上游 edge-tts 7.3.0 同样保留 143，见 rany2/edge-tts#490）。
pub(crate) const SEC_MS_GEC_VERSION: &str = "2-143.0.3650.75";

// 旧 v1 的 WIN_EPOCH/TICK_WINDOW_SECS/INTERVALS_PER_SEC 常量随 v1 移除，见 token_v2.rs。

const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: u64 = 3_600;
const SECS_PER_DAY: u64 = 86_400;
const HOURS_PER_DAY: u64 = 24;
const DAYS_PER_WEEK: usize = 7;
const DAYS_PER_400_YEARS: i64 = 146_097;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days from 0000-03-01 to 1970-01-01 in Howard Hinnant's civil calendar
/// algorithm. See https://howardhinnant.github.io/date_algorithms.html
const DAYS_UNIX_EPOCH_OFFSET: i64 = 719_468;

/// Days in a 4-year Gregorian cycle.
const DAYS_PER_4_YEARS: u64 = 1_460;

/// Days in a 100-year Gregorian cycle.
const DAYS_PER_100_YEARS: u64 = 36_524;

/// Days per year (non-leap).
const DAYS_PER_YEAR: u64 = 365;

/// Numerator coefficient for the month-of-year calculation in Hinnant's algorithm.
const MONTH_NUMERATOR_COEFF: u64 = 153;

/// Small numerator in the month-of-year division.
const MONTH_SCALE: u64 = 5;

/// Constant offset in both month-of-year divisions.
const MONTH_DIVISOR: u64 = 2;

/// Broken-down UTC date-time used to build the `X-Timestamp` header.
pub struct CivilDateTime {
    pub weekday: usize,
    pub year: u64,
    pub month: usize,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    pub second: u64,
}

/// Generate a random uppercase-hex string of `nbytes * 2` hex digits.
///
/// Used for the `MUID` cookie, `ConnectionId`, and `X-RequestId` values.
pub fn random_hex(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02X}")).collect()
}

/// Current Unix timestamp adjusted by `skew` seconds (0 when clocks agree).
fn unix_secs(skew: i64) -> u64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    (secs + skew).max(0) as u64
}

/// Compute the `Sec-MS-GEC` token for the given clock skew.
///
/// 2026-10 起微软要求 v2 令牌（v1 一律 403，rany2/edge-tts#490）：
/// 算法为 CS64 + 调整版 SHA-256 组合，见 [`crate::token_v2`]（移植自
/// rany2/edge-tts 7.3.0 的 drm.py）。
pub fn sec_ms_gec(skew: i64) -> String {
    crate::token_v2::generate_sec_ms_gec_v2(unix_secs(skew) as f64)
}

/// Convert a Unix timestamp (UTC) into a broken-down date-time.
///
/// Uses Howard Hinnant's `days_from_civil` inverse algorithm - no external
/// date crate required.
pub fn civil_utc(secs: u64) -> CivilDateTime {
    let second = secs % SECS_PER_MINUTE;
    let minute = (secs / SECS_PER_MINUTE) % SECS_PER_MINUTE;
    let hour = (secs / SECS_PER_HOUR) % HOURS_PER_DAY;
    let days = (secs / SECS_PER_DAY) as i64;

    // 1970-01-01 was a Thursday (index 4).
    let weekday = ((days + 4).rem_euclid(DAYS_PER_WEEK as i64)) as usize;

    let z = days + DAYS_UNIX_EPOCH_OFFSET;
    let era = if z >= 0 {
        z / DAYS_PER_400_YEARS
    } else {
        (z - (DAYS_PER_400_YEARS - 1)) / DAYS_PER_400_YEARS
    };
    let doe = (z - era * DAYS_PER_400_YEARS) as u64;
    let yoe = (doe - doe / DAYS_PER_4_YEARS + doe / DAYS_PER_100_YEARS
        - doe / DAYS_PER_400_YEARS as u64)
        / DAYS_PER_YEAR;
    let y = yoe as i64 + era * 400;
    let doy = doe - (DAYS_PER_YEAR * yoe + yoe / 4 - yoe / 100);
    let mp = (MONTH_SCALE * doy + MONTH_DIVISOR) / MONTH_NUMERATOR_COEFF;
    let day = doy - (MONTH_NUMERATOR_COEFF * mp + MONTH_DIVISOR) / MONTH_SCALE + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    let year = (y + if month <= 2 { 1 } else { 0 }) as u64;

    CivilDateTime {
        weekday,
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Format a Unix timestamp as a JavaScript-style date string (Edge
/// `X-Timestamp` header format).  Pure - no clock access.
pub fn date_string(secs: u64) -> String {
    let dt = civil_utc(secs);
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS.get(dt.weekday).copied().unwrap_or("Sun"),
        MONTHS
            .get(dt.month.wrapping_sub(1))
            .copied()
            .unwrap_or("Jan"),
        dt.day,
        dt.year,
        dt.hour,
        dt.minute,
        dt.second,
    )
}

/// Current time formatted for the `X-Timestamp` header.
pub(crate) fn current_date_string() -> String {
    date_string(unix_secs(0))
}
