//! Epoch-millisecond → RFC3339 UTC rendering, shared by every provider adapter
//! whose format stores message times as integers.
//!
//! This lives in ports rather than in one adapter because three providers need
//! the identical conversion (opencode `message.time_created`, kimi `time`,
//! cline `timestamp`), and the search time filter is unforgiving: `--since` /
//! `--until` push down to SQL as `sort_key(timestamp) >= ?`, NULL fails every
//! SQL comparison, and the application's `parse_search_instant` only accepts a
//! timezone-qualified RFC3339 string. An epoch integer must therefore be
//! *rendered* — passing it through, or substituting an empty string, silently
//! excludes the message from every time-window search.

/// Upper sanity bound for an epoch-millisecond timestamp: 9999-12-31T23:59:59.999Z.
///
/// Beyond this the year no longer fits RFC3339's four-digit field, so the value
/// is treated as corrupt rather than reformatted.
pub const MAX_EPOCH_MILLIS: i64 = 253_402_300_799_999;

/// Format an epoch-millisecond instant as an RFC3339 UTC timestamp.
///
/// Non-positive and out-of-range values yield `None`: an absent timestamp is
/// honest, a fabricated 1970 one is not. The workspace has no date/time
/// dependency, so the civil-date conversion is done inline.
pub fn rfc3339_utc_from_epoch_millis(millis: i64) -> Option<String> {
    if millis <= 0 || millis > MAX_EPOCH_MILLIS {
        return None;
    }
    let seconds = millis.div_euclid(1_000);
    let subsecond_millis = millis.rem_euclid(1_000);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let second_of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60,
    );
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{subsecond_millis:03}Z"
    ))
}

/// Civil date for a count of days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`) — the inverse of `days_from_civil`, which the application
/// crate uses to parse the filter bounds these timestamps are compared against.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365; // [0, 399]
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100); // [0, 365]
    let shifted_month = (5 * day_of_year + 2) / 153; // [0, 11]
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32; // [1, 31]
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    }; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month as u32, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_millis_render_as_rfc3339_utc() {
        assert_eq!(
            rfc3339_utc_from_epoch_millis(1_771_060_504_250).as_deref(),
            Some("2026-02-14T09:15:04.250Z")
        );
        // Leap day: civil_from_days must agree with days_from_civil.
        assert_eq!(
            rfc3339_utc_from_epoch_millis(951_782_400_000).as_deref(),
            Some("2000-02-29T00:00:00.000Z")
        );
        assert_eq!(
            rfc3339_utc_from_epoch_millis(1).as_deref(),
            Some("1970-01-01T00:00:00.001Z")
        );
        assert_eq!(
            rfc3339_utc_from_epoch_millis(MAX_EPOCH_MILLIS).as_deref(),
            Some("9999-12-31T23:59:59.999Z")
        );
    }

    #[test]
    fn non_positive_or_absurd_epoch_millis_yield_no_timestamp() {
        // Better no timestamp than a fabricated one.
        assert!(rfc3339_utc_from_epoch_millis(0).is_none());
        assert!(rfc3339_utc_from_epoch_millis(-1).is_none());
        assert!(rfc3339_utc_from_epoch_millis(MAX_EPOCH_MILLIS + 1).is_none());
        assert!(rfc3339_utc_from_epoch_millis(i64::MAX).is_none());
    }

    /// Century and leap-century boundaries: the years where a naive
    /// day-count conversion drifts by a day.
    #[test]
    fn century_boundaries_render_exactly() {
        // 2000-01-01 (leap century) and 2100-03-01 (non-leap century).
        assert_eq!(
            rfc3339_utc_from_epoch_millis(946_684_800_000).as_deref(),
            Some("2000-01-01T00:00:00.000Z")
        );
        assert_eq!(
            rfc3339_utc_from_epoch_millis(4_107_542_400_000).as_deref(),
            Some("2100-03-01T00:00:00.000Z")
        );
    }
}
