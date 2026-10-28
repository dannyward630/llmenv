//! Clock-time reset labels for the usage windows (`display: clock_24h` and
//! `display: clock_12h`). The caller passes the time zone. The widget passes
//! the system zone, because a user compares the label with their own clock.

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Clock style for a reset label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStyle {
    /// `04:20`.
    TwentyFourHour,
    /// `4:20am`, `12:40pm`.
    TwelveHour,
}

/// Format the reset moment `resets_at` (Unix seconds) as a lowercase clock
/// label in `tz`. `with_weekday` adds the short weekday first (`mon 04:20`).
/// Returns `None` when `resets_at` is outside the range jiff can represent.
#[must_use]
pub fn format_reset_clock(
    resets_at: i64,
    tz: &TimeZone,
    style: ClockStyle,
    with_weekday: bool,
) -> Option<String> {
    let zoned = Timestamp::from_second(resets_at).ok()?.to_zoned(tz.clone());
    let clock = match style {
        ClockStyle::TwentyFourHour => zoned.strftime("%H:%M").to_string(),
        ClockStyle::TwelveHour => zoned.strftime("%-I:%M%P").to_string(),
    };
    let label = if with_weekday {
        format!("{} {clock}", zoned.strftime("%a"))
    } else {
        clock
    };
    Some(label.to_lowercase())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "a test fails loudly when its fixture cannot be set up"
)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Epoch seconds for an RFC 3339 UTC literal. The literals are fixed in
    /// the tests, so a parse failure is a test bug.
    fn at(rfc3339: &str) -> i64 {
        rfc3339
            .parse::<Timestamp>()
            .map(|t| t.as_second())
            .expect("test literal is RFC 3339")
    }

    #[test]
    fn twenty_four_hour_clock_is_zero_padded() {
        let t = at("2026-10-26T04:20:00Z");
        assert_eq!(
            format_reset_clock(t, &TimeZone::UTC, ClockStyle::TwentyFourHour, false).as_deref(),
            Some("04:20")
        );
    }

    #[test]
    fn twelve_hour_clock_uses_lowercase_meridiem() {
        let t = at("2026-10-26T12:40:00Z");
        assert_eq!(
            format_reset_clock(t, &TimeZone::UTC, ClockStyle::TwelveHour, false).as_deref(),
            Some("12:40pm")
        );
    }

    #[test]
    fn twelve_hour_midnight_reads_as_twelve_am() {
        let t = at("2026-10-26T00:05:00Z");
        assert_eq!(
            format_reset_clock(t, &TimeZone::UTC, ClockStyle::TwelveHour, false).as_deref(),
            Some("12:05am")
        );
    }

    #[test]
    fn weekday_prefix_for_a_long_window() {
        let t = at("2026-10-26T04:20:00Z");
        assert_eq!(
            format_reset_clock(t, &TimeZone::UTC, ClockStyle::TwentyFourHour, true).as_deref(),
            Some("mon 04:20")
        );
    }

    #[test]
    fn out_of_range_epoch_yields_none() {
        assert_eq!(
            format_reset_clock(i64::MAX, &TimeZone::UTC, ClockStyle::TwelveHour, true),
            None
        );
    }

    proptest! {
        /// Any epoch gives a lowercase label, and `None` exactly when jiff cannot represent it.
        #[test]
        fn label_has_the_expected_shape(
            resets_at in any::<i64>(),
            twelve_hour in any::<bool>(),
            with_weekday in any::<bool>(),
        ) {
            let style = if twelve_hour {
                ClockStyle::TwelveHour
            } else {
                ClockStyle::TwentyFourHour
            };
            let label = format_reset_clock(resets_at, &TimeZone::UTC, style, with_weekday);
            prop_assert_eq!(label.is_some(), Timestamp::from_second(resets_at).is_ok());
            if let Some(label) = label {
                prop_assert_eq!(label.clone(), label.to_lowercase());
                let clock = if with_weekday {
                    let (day, clock) = label.split_once(' ').expect("weekday then clock");
                    prop_assert!(["mon", "tue", "wed", "thu", "fri", "sat", "sun"].contains(&day));
                    clock
                } else {
                    label.as_str()
                };
                let time = if twelve_hour {
                    let (time, meridiem) = clock.split_at(clock.len() - 2);
                    prop_assert!(meridiem == "am" || meridiem == "pm");
                    time
                } else {
                    clock
                };
                prop_assert!(time.len() == 4 || time.len() == 5, "{label}");
                prop_assert!(time.bytes().all(|b| b.is_ascii_digit() || b == b':'), "{label}");
            }
        }
    }
}
