//! Dates, as the number Excel keeps underneath one.
//!
//! There is no date type in a spreadsheet. A date is a **number of days since
//! an epoch**, shown as a date because the cell carries a date number format;
//! the time of day is the fractional part. Change the format and the same cell
//! reads 46237.
//!
//! # The leap year that never happened
//!
//! Excel believes 29 February 1900 existed. It did not — 1900 was not a leap
//! year — and the belief is deliberate: Lotus 1-2-3 had the bug, and in 1985
//! compatibility with Lotus was worth more than a correct calendar. Every
//! spreadsheet since has kept it, because serial numbers are stored in files
//! that already exist.
//!
//! So the arithmetic has a seam at serial 60, which is the day that was never
//! a day. Before it, one epoch; after it, another. Everything a business
//! actually records falls after the seam, which is exactly why it is worth
//! handling rather than assuming: the one date that trips it is the sentinel
//! somebody used for "no date", and it will be in the first export.

/// Days from 1899-12-30 to 1970-01-01, which is where a Unix timestamp starts.
const UNIX_EPOCH_SERIAL: f64 = 25_569.0;

const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;

/// The serial for 1900-03-01, the first day after the phantom one.
const FIRST_DAY_AFTER_THE_PHANTOM: i64 = 61;

/// The Excel serial for a calendar date.
///
/// Returns `None` for a date that is not a date — month 13, the 31st of
/// February — rather than silently rolling it forward into a plausible wrong
/// answer, and for anything before 1900, which a spreadsheet cannot represent
/// at all.
pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<f64> {
    if !is_a_real_date(year, month, day) {
        return None;
    }

    // Counted from 1899-12-30 rather than 1900-01-01, which is what makes the
    // common case a plain subtraction: the two-day offset absorbs both the
    // epoch and the phantom day.
    let days = days_from_civil(year, month, day) - days_from_civil(1899, 12, 30);

    // Before the phantom day, Excel is one ahead of the calendar, because it
    // has not yet counted a day that does not exist.
    let serial = if days < FIRST_DAY_AFTER_THE_PHANTOM {
        days - 1
    } else {
        days
    };

    (serial >= 1).then_some(serial as f64)
}

/// As [`from_ymd`], with a time of day in the fractional part.
pub fn from_ymd_hms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<f64> {
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let seconds = (hour * 3600 + minute * 60 + second) as f64;
    Some(from_ymd(year, month, day)? + seconds / 86_400.0)
}

/// The Excel serial for a Unix timestamp in milliseconds.
///
/// The instant is read as UTC, because a Unix timestamp names an instant and
/// nothing else. A serial names a wall clock with no zone at all, so somebody
/// has to choose which wall clock — and that is the producer, who knows
/// whether the date came from a database in UTC or from a person in Madrid.
/// Choosing here would mean guessing, and guessing wrong puts a transaction on
/// the previous day.
pub fn from_unix_ms(milliseconds: f64) -> f64 {
    milliseconds / MILLISECONDS_PER_DAY + UNIX_EPOCH_SERIAL
}

fn is_a_real_date(year: i32, month: u32, day: u32) -> bool {
    if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    day <= days_in_month(year, month)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        // The real rule, not Excel's. February 1900 had 28 days here and 29 in
        // the serial numbering, and both statements are correct in their own
        // terms: `from_ymd(1900, 2, 29)` is not a date, and serial 60 is what
        // Excel would show if it were.
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's algorithm.
///
/// Shifts the year to start in March so that the leap day lands at the end of
/// it, which removes every special case from the month arithmetic.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = (year - era * 400) as i64;
    let month = month as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era as i64 * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values taken from Python's `datetime`, counting the way Excel
    /// counts. An independent calendar, so the test is not the code restated.
    #[test]
    fn matches_excel_on_dates_either_side_of_the_phantom_day() {
        assert_eq!(from_ymd(1900, 1, 1), Some(1.0));
        assert_eq!(from_ymd(1900, 2, 28), Some(59.0));
        // 60 is 1900-02-29, which never happened. Nothing maps to it.
        assert_eq!(from_ymd(1900, 3, 1), Some(61.0));
        assert_eq!(from_ymd(1901, 1, 1), Some(367.0));
    }

    #[test]
    fn matches_excel_on_dates_anybody_would_actually_type() {
        assert_eq!(from_ymd(1970, 1, 1), Some(25_569.0));
        assert_eq!(from_ymd(2000, 1, 1), Some(36_526.0));
        assert_eq!(from_ymd(2026, 8, 3), Some(46_237.0));
    }

    #[test]
    fn no_date_maps_onto_the_day_that_never_happened() {
        // Serial 60 is reachable only by a file that already contains it.
        // Nothing this crate writes can land there, which means a round trip
        // through our own conversion can never invent 29 February 1900.
        for day in 1..=31 {
            for month in 1..=12 {
                assert_ne!(from_ymd(1900, month, day), Some(60.0), "{month}/{day}");
            }
        }
    }

    #[test]
    fn february_the_twenty_ninth_of_1900_is_not_a_date() {
        // Excel's numbering has the day; the calendar does not. Asked for it
        // by name, the honest answer is that there is no such date.
        assert_eq!(from_ymd(1900, 2, 29), None);
    }

    #[test]
    fn keeps_the_leap_years_that_are_real() {
        assert!(from_ymd(2024, 2, 29).is_some());
        assert_eq!(from_ymd(2023, 2, 29), None);
        // 2000 was a leap year; 1900 and 2100 are not. The century rule is the
        // one people get wrong, and a spreadsheet of contract dates will find it.
        assert!(from_ymd(2000, 2, 29).is_some());
        assert_eq!(from_ymd(2100, 2, 29), None);
    }

    #[test]
    fn refuses_what_is_not_a_date_rather_than_rolling_it_forward() {
        // Rolling 31 April to 1 May is what a lenient parser does, and it puts
        // an invoice in the wrong month without saying anything.
        assert_eq!(from_ymd(2026, 4, 31), None);
        assert_eq!(from_ymd(2026, 13, 1), None);
        assert_eq!(from_ymd(2026, 0, 1), None);
        assert_eq!(from_ymd(2026, 1, 0), None);
    }

    #[test]
    fn refuses_dates_a_spreadsheet_cannot_hold() {
        // Excel's numbering begins at 1900-01-01. A birth date in 1887 is a
        // real date and not a serial number, and pretending otherwise writes a
        // negative number that displays as garbage.
        assert_eq!(from_ymd(1899, 12, 31), None);
        assert_eq!(from_ymd(1800, 1, 1), None);
    }

    #[test]
    fn a_time_of_day_is_the_fraction_after_the_point() {
        let noon = from_ymd_hms(2026, 8, 3, 12, 0, 0).expect("a real date and time");
        assert_eq!(noon, 46_237.5);

        let quarter_past = from_ymd_hms(2026, 8, 3, 6, 0, 0).expect("a real date and time");
        assert_eq!(quarter_past, 46_237.25);
    }

    #[test]
    fn midnight_is_the_date_itself() {
        assert_eq!(from_ymd_hms(2026, 8, 3, 0, 0, 0), Some(46_237.0));
    }

    #[test]
    fn refuses_a_time_that_is_not_a_time() {
        assert_eq!(from_ymd_hms(2026, 8, 3, 24, 0, 0), None);
        assert_eq!(from_ymd_hms(2026, 8, 3, 0, 60, 0), None);
    }

    #[test]
    fn converts_a_unix_timestamp_read_as_utc() {
        // The Unix epoch itself, and a day after it.
        assert_eq!(from_unix_ms(0.0), 25_569.0);
        assert_eq!(from_unix_ms(86_400_000.0), 25_570.0);
        // Midday on the epoch is the fraction, as with any other time.
        assert_eq!(from_unix_ms(43_200_000.0), 25_569.5);
    }

    #[test]
    fn agrees_with_itself_whichever_way_a_date_arrives() {
        // 2026-08-03T00:00:00Z as milliseconds.
        let ms = 1_785_715_200_000.0;
        assert_eq!(from_unix_ms(ms), from_ymd(2026, 8, 3).unwrap());
    }
}
