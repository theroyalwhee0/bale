use crate::error::BaleError;
use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use std::fmt;
use std::time::SystemTime;

/// MS-DOS date/time format used in ZIP archives.
///
/// Date bits: `YYYYYYYM MMMDDDDD`
/// - Bits 0-4: Day (1-31)
/// - Bits 5-8: Month (1-12)
/// - Bits 9-15: Year offset from 1980 (0-127)
///
/// Time bits: `HHHHHMMM MMMSSSSS`
/// - Bits 0-4: Seconds/2 (0-29, representing 0-58)
/// - Bits 5-10: Minutes (0-59)
/// - Bits 11-15: Hours (0-23)
///
/// # Ordering
///
/// The derived `Ord` implementation produces chronological ordering because:
/// 1. Fields are ordered `date` then `time`
/// 2. Both fields pack most-significant time units in higher bits
///
/// **Note:** Do not reorder the struct fields without updating `Ord`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DosDateTime {
    /// MS-DOS date field. Must be first for correct `Ord` derivation.
    pub date: u16,
    /// MS-DOS time field. Must be second for correct `Ord` derivation.
    pub time: u16,
}

impl Default for DosDateTime {
    /// Returns the DOS epoch (1980-01-01 00:00:00).
    fn default() -> Self {
        Self {
            date: Self::DOS_EPOCH_DATE,
            time: Self::MIDNIGHT,
        }
    }
}

impl DosDateTime {
    /// The MS-DOS epoch year.
    const DOS_EPOCH_YEAR: u16 = 1980;

    /// Maximum year offset from DOS epoch (7 bits: 0-127, representing 1980-2107).
    const MAX_YEAR_OFFSET: u16 = 127;

    // Bit masks for extracting date/time components.
    /// Mask for year offset (7 bits, positions 9-15 of date field).
    const YEAR_MASK: u16 = 0x7F;
    /// Mask for month (4 bits, positions 5-8 of date field).
    const MONTH_MASK: u16 = 0x0F;
    /// Mask for day (5 bits, positions 0-4 of date field).
    const DAY_MASK: u16 = 0x1F;
    /// Mask for hour (5 bits, positions 11-15 of time field).
    const HOUR_MASK: u16 = 0x1F;
    /// Mask for minute (6 bits, positions 5-10 of time field).
    const MINUTE_MASK: u16 = 0x3F;
    /// Mask for seconds/2 (5 bits, positions 0-4 of time field).
    const SECOND_MASK: u16 = 0x1F;

    // DOS epoch encoded values.
    /// DOS epoch date: 1980-01-01 encoded as (0 << 9) | (1 << 5) | 1.
    const DOS_EPOCH_DATE: u16 = 0x0021;
    /// Midnight time: 00:00:00 encoded as 0.
    const MIDNIGHT: u16 = 0x0000;

    /// Creates a new `DosDateTime` from date and time fields.
    #[must_use]
    pub const fn new(date: u16, time: u16) -> Self {
        Self { date, time }
    }

    /// Creates a `DosDateTime` from individual components.
    ///
    /// Validates the components and returns an error if the date/time is invalid.
    /// Seconds are truncated to 2-second resolution (e.g., 59 becomes 58).
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidDosDateTime` if any component is out of range:
    /// - Year must be 1980-2107
    /// - Month must be 1-12
    /// - Day must be valid for the given month/year
    /// - Hour must be 0-23
    /// - Minute must be 0-59
    /// - Second must be 0-59
    pub fn from_components(
        year: u16,
        month: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
    ) -> Result<Self, BaleError> {
        // Validate all components BEFORE doing any calculations.
        // This prevents invalid inputs from wrapping into valid-looking results.
        let valid = (Self::DOS_EPOCH_YEAR..=Self::DOS_EPOCH_YEAR + Self::MAX_YEAR_OFFSET)
            .contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= Self::days_in_month(year, month)
            && hour <= 23
            && minute <= 59
            && second <= 59; // Accepts 0-59; truncated to 0-29 (even seconds 0-58) below.

        if !valid {
            // Construct the invalid date for the error message, clamping to avoid overflow.
            let year_offset = year
                .saturating_sub(Self::DOS_EPOCH_YEAR)
                .min(Self::MAX_YEAR_OFFSET);
            let date =
                (year_offset << 9) | ((month & Self::MONTH_MASK) << 5) | (day & Self::DAY_MASK);
            let time = ((hour & Self::HOUR_MASK) << 11)
                | ((minute & Self::MINUTE_MASK) << 5)
                | ((second / 2) & Self::SECOND_MASK);
            return Err(BaleError::InvalidDosDateTime(Self { date, time }));
        }

        // Safe to calculate now - all components are validated.
        let year_offset = year - Self::DOS_EPOCH_YEAR;
        let date = (year_offset << 9) | (month << 5) | day;
        // Truncate seconds to 2-second resolution: 59 -> 29, which displays as 58.
        let time = (hour << 11) | (minute << 5) | (second / 2);

        Ok(Self { date, time })
    }

    /// Extracts the year (1980-2107).
    #[inline]
    #[must_use]
    pub const fn year(&self) -> u16 {
        ((self.date >> 9) & Self::YEAR_MASK) + Self::DOS_EPOCH_YEAR
    }

    /// Extracts the month (1-12).
    #[inline]
    #[must_use]
    pub const fn month(&self) -> u16 {
        (self.date >> 5) & Self::MONTH_MASK
    }

    /// Extracts the day (1-31).
    #[inline]
    #[must_use]
    pub const fn day(&self) -> u16 {
        self.date & Self::DAY_MASK
    }

    /// Extracts the hour (0-23).
    #[inline]
    #[must_use]
    pub const fn hour(&self) -> u16 {
        (self.time >> 11) & Self::HOUR_MASK
    }

    /// Extracts the minute (0-59).
    #[inline]
    #[must_use]
    pub const fn minute(&self) -> u16 {
        (self.time >> 5) & Self::MINUTE_MASK
    }

    /// Extracts the second (0-58, always even).
    #[inline]
    #[must_use]
    pub const fn second(&self) -> u16 {
        (self.time & Self::SECOND_MASK) * 2
    }

    /// Returns the number of days in the given month for the given year.
    #[must_use]
    const fn days_in_month(year: u16, month: u16) -> u16 {
        match month {
            // Months with 31 days.
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            // Months with 30 days.
            4 | 6 | 9 | 11 => 30,
            // February.
            2 => {
                // Leap year: divisible by 4, except centuries unless divisible by 400.
                let is_leap = year.is_multiple_of(4)
                    && (!year.is_multiple_of(100) || year.is_multiple_of(400));
                if is_leap { 29 } else { 28 }
            }
            // Other.
            _ => 0,
        }
    }

    /// Returns `true` if this represents a valid date and time.
    ///
    /// Checks that month, day, hour, minute, and second are within valid ranges,
    /// including month-specific day limits (e.g., no February 30).
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        let month = self.month();
        let day = self.day();
        let hour = self.hour();
        let minute = self.minute();
        // Raw seconds field (0-29) before doubling.
        let second_raw = self.time & Self::SECOND_MASK;

        // Validate ranges.
        if month < 1 || month > 12 {
            return false;
        }
        if day < 1 || day > Self::days_in_month(self.year(), month) {
            return false;
        }
        if hour > 23 {
            return false;
        }
        if minute > 59 {
            return false;
        }
        // Seconds are stored as 5 bits (0-31), representing 0-62 seconds in 2-second
        // increments. Valid values are 0-29 (0-58 seconds). Values 30-31 would represent
        // 60-62 seconds which are invalid. Note: from_components() accepts 0-59 and
        // truncates (e.g., 59 -> 29 -> displays as 58), so valid inputs always produce
        // valid stored values.
        if second_raw > 29 {
            return false;
        }

        true
    }

    /// Converts to `SystemTime`, returning an error if the date/time is invalid.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidDosDateTime` if the date/time components are
    /// invalid (e.g., month=0, February 30).
    pub fn to_system_time(&self) -> Result<SystemTime, BaleError> {
        SystemTime::try_from(*self)
    }

    /// Converts to `SystemTime`, falling back to DOS epoch if invalid.
    ///
    /// This is a convenience method for cases where you want a valid `SystemTime`
    /// regardless of whether the DOS date/time is valid.
    ///
    /// # Panics
    ///
    /// This function will not panic. The internal `expect` is for the DOS epoch
    /// constant (1980-01-01 00:00:00), which is always valid.
    ///
    /// # Example
    ///
    /// ```
    /// use bale::DosDateTime;
    ///
    /// let dos = DosDateTime::default(); // 1980-01-01 00:00:00
    /// let system_time = dos.to_system_time_or_epoch();
    /// ```
    #[must_use]
    pub fn to_system_time_or_epoch(&self) -> SystemTime {
        self.to_system_time().unwrap_or_else(|_| {
            Utc.with_ymd_and_hms(Self::DOS_EPOCH_YEAR as i32, 1, 1, 0, 0, 0)
                .single()
                .expect("DOS epoch is always valid")
                .into()
        })
    }
}

/// Implement `Display` for `DosDateTime`.
impl fmt::Display for DosDateTime {
    /// Formats the DOS date/time as `YYYY-MM-DD HH:MM:SS`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year(),
            self.month(),
            self.day(),
            self.hour(),
            self.minute(),
            self.second()
        )
    }
}

/// Implement `From<SystemTime>` for `DosDateTime`.
impl From<SystemTime> for DosDateTime {
    /// Converts a `SystemTime` to MS-DOS date/time format.
    ///
    /// Dates before 1980 clamp to 1980-01-01 00:00:00.
    /// Dates after 2107 clamp to 2107-12-31 23:59:58.
    /// Seconds are truncated to 2-second resolution (odd seconds round down).
    fn from(time: SystemTime) -> Self {
        let dt: DateTime<Utc> = time.into();
        let input_year = dt.year();

        // Clamp to entire boundary dates to avoid invalid combinations
        // (e.g., 2150-02-29 clamped to 2107-02-29 would be invalid).
        let (year, month, day, hour, minute, second) = if input_year < Self::DOS_EPOCH_YEAR as i32 {
            // Before DOS epoch: clamp to 1980-01-01 00:00:00.
            (0u16, 1u16, 1u16, 0u16, 0u16, 0u16)
        } else if input_year > (Self::DOS_EPOCH_YEAR + Self::MAX_YEAR_OFFSET) as i32 {
            // After max DOS year: clamp to 2107-12-31 23:59:58.
            (Self::MAX_YEAR_OFFSET, 12u16, 31u16, 23u16, 59u16, 58u16)
        } else {
            // Within valid range: use actual values.
            (
                (input_year - Self::DOS_EPOCH_YEAR as i32) as u16,
                dt.month() as u16,
                dt.day() as u16,
                dt.hour() as u16,
                dt.minute() as u16,
                dt.second() as u16,
            )
        };

        // Pack date: YYYYYYYM MMMDDDDD (year bits 15-9, month bits 8-5, day bits 4-0).
        let date = (year << 9) | (month << 5) | day;
        // Pack time: HHHHHMMM MMMSSSSS (hour bits 15-11, minute bits 10-5, seconds/2 bits 4-0).
        let time = (hour << 11) | (minute << 5) | (second / 2);

        Self { date, time }
    }
}

/// Implement `TryFrom<DosDateTime>` for `SystemTime`.
impl TryFrom<DosDateTime> for SystemTime {
    type Error = BaleError;

    /// Converts MS-DOS date/time to a `SystemTime`.
    ///
    /// # Errors
    ///
    /// Returns `BaleError::InvalidDosDateTime` if the date/time components are
    /// invalid (e.g., month=0, Feb 30).
    ///
    /// # Example
    ///
    /// ```
    /// use std::time::SystemTime;
    /// use bale::DosDateTime;
    ///
    /// let dos = DosDateTime::default(); // 1980-01-01 00:00:00
    /// let system_time = SystemTime::try_from(dos).expect("valid date");
    ///
    /// // Or use the convenience method for fallback:
    /// let system_time = dos.to_system_time_or_epoch();
    /// ```
    fn try_from(dos: DosDateTime) -> Result<Self, Self::Error> {
        Utc.with_ymd_and_hms(
            dos.year() as i32,
            dos.month() as u32,
            dos.day() as u32,
            dos.hour() as u32,
            dos.minute() as u32,
            dos.second() as u32,
        )
        .single()
        .map(Into::into)
        .ok_or(BaleError::InvalidDosDateTime(dos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

    /// The DOS epoch (1980-01-01 00:00:00) encodes correctly.
    #[test]
    fn dos_epoch() {
        let dos = DosDateTime::new(0x0021, 0x0000); // 1980-01-01 00:00:00
        assert_eq!(dos.year(), 1980);
        assert_eq!(dos.month(), 1);
        assert_eq!(dos.day(), 1);
        assert_eq!(dos.hour(), 0);
        assert_eq!(dos.minute(), 0);
        assert_eq!(dos.second(), 0);
        assert!(dos.is_valid());
    }

    /// `Default` returns a valid DOS epoch date.
    #[test]
    fn default_is_valid_epoch() {
        let dos = DosDateTime::default();
        assert_eq!(dos.year(), 1980);
        assert_eq!(dos.month(), 1);
        assert_eq!(dos.day(), 1);
        assert_eq!(dos.hour(), 0);
        assert_eq!(dos.minute(), 0);
        assert_eq!(dos.second(), 0);
        assert!(dos.is_valid());
    }

    /// `DosDateTime` can be used in hash collections.
    #[test]
    fn hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DosDateTime::new(0x0021, 0x0000));
        set.insert(DosDateTime::new(0x0021, 0x0000)); // Duplicate
        set.insert(DosDateTime::new(0x5921, 0x6B15)); // 2024-06-15 13:10:42
        assert_eq!(set.len(), 2);
    }

    /// SystemTime -> DosDateTime -> SystemTime preserves date/time components.
    #[test]
    fn roundtrip() {
        // 2024-06-15 13:10:42 UTC
        let original = Utc.with_ymd_and_hms(2024, 6, 15, 13, 10, 42).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        let restored: SystemTime = dos.try_into().expect("valid date");
        let restored_dt: DateTime<Utc> = restored.into();

        assert!(dos.is_valid());

        // DOS time has 2-second resolution.
        assert_eq!(restored_dt.year(), 2024);
        assert_eq!(restored_dt.month(), 6);
        assert_eq!(restored_dt.day(), 15);
        assert_eq!(restored_dt.hour(), 13);
        assert_eq!(restored_dt.minute(), 10);
        assert_eq!(restored_dt.second(), 42);
    }

    /// Component extraction methods return correct values.
    #[test]
    fn extract_components() {
        let original = Utc.with_ymd_and_hms(2024, 6, 15, 13, 10, 42).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        assert_eq!(dos.year(), 2024);
        assert_eq!(dos.month(), 6);
        assert_eq!(dos.day(), 15);
        assert_eq!(dos.hour(), 13);
        assert_eq!(dos.minute(), 10);
        assert_eq!(dos.second(), 42);
        assert!(dos.is_valid());
    }

    /// Dates before DOS epoch (1980) clamp to 1980-01-01 00:00:00.
    #[test]
    fn before_dos_epoch() {
        // 1970-01-01 should clamp to 1980-01-01 00:00:00
        let original = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        assert_eq!(dos.year(), 1980);
        assert_eq!(dos.month(), 1);
        assert_eq!(dos.day(), 1);
        assert_eq!(dos.hour(), 0);
        assert_eq!(dos.minute(), 0);
        assert_eq!(dos.second(), 0);
        assert!(dos.is_valid());
    }

    /// Dates after max DOS year (2107) clamp to 2107-12-31 23:59:58.
    #[test]
    fn after_dos_max_year() {
        // 2150-02-29 would be invalid if only year was clamped to 2107
        // (2107 is not a leap year), so entire date must clamp.
        let original = Utc.with_ymd_and_hms(2150, 6, 15, 14, 30, 45).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        assert_eq!(dos.year(), 2107);
        assert_eq!(dos.month(), 12);
        assert_eq!(dos.day(), 31);
        assert_eq!(dos.hour(), 23);
        assert_eq!(dos.minute(), 59);
        assert_eq!(dos.second(), 58); // Max representable (seconds stored as /2)
        assert!(dos.is_valid());
    }

    /// Valid dates pass `is_valid()`.
    #[test]
    fn is_valid_accepts_valid_dates() {
        // DOS epoch.
        assert!(DosDateTime::new(0x0021, 0x0000).is_valid());
        // Normal date: 2024-06-15 13:10:42.
        let dos = DosDateTime::from(SystemTime::from(
            Utc.with_ymd_and_hms(2024, 6, 15, 13, 10, 42).unwrap(),
        ));
        assert!(dos.is_valid());
        // Leap year Feb 29: 2000-02-29.
        let leap = DosDateTime::from(SystemTime::from(
            Utc.with_ymd_and_hms(2000, 2, 29, 0, 0, 0).unwrap(),
        ));
        assert!(leap.is_valid());
    }

    /// Invalid months fail `is_valid()`.
    #[rstest]
    #[case(0x0001, 0)] // Month 0
    #[case(0x01A1, 13)] // Month 13
    #[case(0x01C1, 14)] // Month 14
    #[case(0x01E1, 15)] // Month 15 (max 4-bit value)
    fn is_valid_rejects_invalid_month(#[case] date: u16, #[case] expected_month: u16) {
        let dos = DosDateTime::new(date, 0x0000);
        assert_eq!(dos.month(), expected_month);
        assert!(!dos.is_valid());
    }

    /// Invalid days fail `is_valid()`.
    #[rstest]
    #[case(0x0020, 1, 0)] // Day 0
    #[case(0x005E, 2, 30)] // Feb 30
    #[case(0x025D, 2, 29)] // Feb 29 non-leap (1981)
    #[case(0x009F, 4, 31)] // Apr 31
    #[case(0x00DF, 6, 31)] // Jun 31
    #[case(0x013F, 9, 31)] // Sep 31
    #[case(0x017F, 11, 31)] // Nov 31
    fn is_valid_rejects_invalid_day(
        #[case] date: u16,
        #[case] expected_month: u16,
        #[case] expected_day: u16,
    ) {
        let dos = DosDateTime::new(date, 0x0000);
        assert_eq!(dos.month(), expected_month);
        assert_eq!(dos.day(), expected_day);
        assert!(!dos.is_valid());
    }

    /// Display formats as `YYYY-MM-DD HH:MM:SS`.
    #[test]
    fn display_format() {
        let dos = DosDateTime::from(SystemTime::from(
            Utc.with_ymd_and_hms(2024, 6, 15, 13, 10, 42).unwrap(),
        ));
        assert_eq!(dos.to_string(), "2024-06-15 13:10:42");
    }

    /// Invalid dates return an error with `TryFrom`.
    #[test]
    fn try_from_invalid_returns_error() {
        // Feb 30 is invalid.
        let dos = DosDateTime::new(0x005E, 0x0000);
        let result = SystemTime::try_from(dos);
        assert!(result.is_err());
    }

    /// `to_system_time_or_epoch()` falls back to DOS epoch for invalid dates.
    #[test]
    fn to_system_time_or_epoch_falls_back() {
        // Feb 30 is invalid.
        let dos = DosDateTime::new(0x005E, 0x0000);
        let time = dos.to_system_time_or_epoch();
        let dt: DateTime<Utc> = time.into();
        // Should fall back to DOS epoch.
        assert_eq!(dt.year(), 1980);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 0);
        assert_eq!(dt.minute(), 0);
        assert_eq!(dt.second(), 0);
    }

    /// Invalid time fields fail `is_valid()`.
    #[rstest]
    #[case(0xC000, 24, 0, 0)] // Hour 24
    #[case(0xF800, 31, 0, 0)] // Hour 31 (max 5-bit value)
    #[case(0x0780, 0, 60, 0)] // Minute 60
    #[case(0x07E0, 0, 63, 0)] // Minute 63 (max 6-bit value)
    #[case(0x001E, 0, 0, 60)] // Second 60 (raw=30)
    #[case(0x001F, 0, 0, 62)] // Second 62 (raw=31, max 5-bit value)
    fn is_valid_rejects_invalid_time(
        #[case] time: u16,
        #[case] expected_hour: u16,
        #[case] expected_minute: u16,
        #[case] expected_second: u16,
    ) {
        let dos = DosDateTime::new(0x0021, time); // Valid date: 1980-01-01
        assert_eq!(dos.hour(), expected_hour);
        assert_eq!(dos.minute(), expected_minute);
        assert_eq!(dos.second(), expected_second);
        assert!(!dos.is_valid());
    }

    /// `days_in_month` returns 0 for invalid months.
    #[rstest]
    #[case(0)]
    #[case(13)]
    #[case(14)]
    #[case(15)]
    #[case(255)]
    fn days_in_month_invalid_month(#[case] month: u16) {
        assert_eq!(DosDateTime::days_in_month(1980, month), 0);
    }

    /// `days_in_month` returns correct values for all months.
    #[rstest]
    #[case(1, 31)] // January
    #[case(2, 28)] // February (non-leap)
    #[case(3, 31)] // March
    #[case(4, 30)] // April
    #[case(5, 31)] // May
    #[case(6, 30)] // June
    #[case(7, 31)] // July
    #[case(8, 31)] // August
    #[case(9, 30)] // September
    #[case(10, 31)] // October
    #[case(11, 30)] // November
    #[case(12, 31)] // December
    fn days_in_month_all_months(#[case] month: u16, #[case] expected: u16) {
        assert_eq!(DosDateTime::days_in_month(1981, month), expected);
    }

    /// Leap year detection for February.
    #[rstest]
    #[case(1980, 29)] // Leap year (divisible by 4)
    #[case(1981, 28)] // Non-leap year
    #[case(2000, 29)] // Century leap year (divisible by 400)
    #[case(2100, 28)] // Century non-leap year (divisible by 100, not 400)
    fn days_in_february_leap_years(#[case] year: u16, #[case] expected: u16) {
        assert_eq!(DosDateTime::days_in_month(year, 2), expected);
    }

    /// Boundary values for time fields.
    #[rstest]
    #[case(0, 0, 0)] // Minimum time
    #[case(23, 59, 58)] // Maximum valid time (58 seconds due to 2-second resolution)
    fn time_boundaries(#[case] hour: u16, #[case] minute: u16, #[case] second: u16) {
        let time = (hour << 11) | (minute << 5) | (second / 2);
        let dos = DosDateTime::new(0x0021, time); // 1980-01-01
        assert_eq!(dos.hour(), hour);
        assert_eq!(dos.minute(), minute);
        assert_eq!(dos.second(), second);
        assert!(dos.is_valid());
    }

    /// Boundary values for date fields.
    #[rstest]
    #[case(1980, 1, 1)] // DOS epoch
    #[case(2107, 12, 31)] // Maximum DOS date
    fn date_boundaries(#[case] year: u16, #[case] month: u16, #[case] day: u16) {
        let year_offset = year - 1980;
        let date = (year_offset << 9) | (month << 5) | day;
        let dos = DosDateTime::new(date, 0x0000);
        assert_eq!(dos.year(), year);
        assert_eq!(dos.month(), month);
        assert_eq!(dos.day(), day);
        assert!(dos.is_valid());
    }

    /// `from_components` creates valid DosDateTime from valid inputs.
    #[rstest]
    #[case(1980, 1, 1, 0, 0, 0)] // DOS epoch
    #[case(2024, 6, 15, 13, 10, 42)] // Normal date
    #[case(2107, 12, 31, 23, 59, 59)] // Maximum values (second 59 truncates to 58)
    #[case(2000, 2, 29, 12, 30, 0)] // Leap year
    fn from_components_valid(
        #[case] year: u16,
        #[case] month: u16,
        #[case] day: u16,
        #[case] hour: u16,
        #[case] minute: u16,
        #[case] second: u16,
    ) {
        let dos = DosDateTime::from_components(year, month, day, hour, minute, second)
            .expect("should create valid DosDateTime");
        assert_eq!(dos.year(), year);
        assert_eq!(dos.month(), month);
        assert_eq!(dos.day(), day);
        assert_eq!(dos.hour(), hour);
        assert_eq!(dos.minute(), minute);
        // Second is truncated to 2-second resolution
        assert_eq!(dos.second(), second / 2 * 2);
        assert!(dos.is_valid());
    }

    /// Odd seconds are truncated to even values (2-second resolution).
    #[rstest]
    #[case(1, 0)]
    #[case(3, 2)]
    #[case(59, 58)]
    fn odd_second_truncation(#[case] input: u16, #[case] expected: u16) {
        let dos = DosDateTime::from_components(2000, 1, 1, 0, 0, input).expect("valid date");
        assert_eq!(
            dos.second(),
            expected,
            "input second {} should truncate to {}",
            input,
            expected
        );
    }

    /// `from_components` rejects invalid inputs.
    #[rstest]
    #[case(1979, 1, 1, 0, 0, 0)] // Year before epoch
    #[case(2108, 1, 1, 0, 0, 0)] // Year after max
    #[case(2000, 0, 1, 0, 0, 0)] // Month 0
    #[case(2000, 13, 1, 0, 0, 0)] // Month 13
    #[case(2000, 1, 0, 0, 0, 0)] // Day 0
    #[case(2000, 1, 32, 0, 0, 0)] // Day 32
    #[case(2000, 2, 30, 0, 0, 0)] // Feb 30
    #[case(2001, 2, 29, 0, 0, 0)] // Feb 29 in non-leap year
    #[case(2000, 1, 1, 24, 0, 0)] // Hour 24
    #[case(2000, 1, 1, 0, 60, 0)] // Minute 60
    #[case(2000, 1, 1, 0, 0, 60)] // Second 60
    fn from_components_invalid(
        #[case] year: u16,
        #[case] month: u16,
        #[case] day: u16,
        #[case] hour: u16,
        #[case] minute: u16,
        #[case] second: u16,
    ) {
        let result = DosDateTime::from_components(year, month, day, hour, minute, second);
        assert!(
            result.is_err(),
            "should reject {}-{:02}-{:02} {:02}:{:02}:{:02}",
            year,
            month,
            day,
            hour,
            minute,
            second
        );
    }

    proptest! {
        /// Round-trip: SystemTime -> DosDateTime -> SystemTime preserves components.
        #[test]
        fn roundtrip_proptest(
            year in 1980u16..=2107,
            month in 1u16..=12,
            day in 1u16..=28, // Use 28 to avoid invalid dates
            hour in 0u16..=23,
            minute in 0u16..=59,
            second in (0u16..=29).prop_map(|s| s * 2), // Even seconds only
        ) {
            let original = Utc
                .with_ymd_and_hms(year as i32, month as u32, day as u32, hour as u32, minute as u32, second as u32)
                .single()
                .unwrap();
            let dos = DosDateTime::from(SystemTime::from(original));
            let restored: SystemTime = dos.try_into().expect("valid date");
            let restored_dt: DateTime<Utc> = restored.into();

            prop_assert_eq!(restored_dt.year() as u16, year);
            prop_assert_eq!(restored_dt.month() as u16, month);
            prop_assert_eq!(restored_dt.day() as u16, day);
            prop_assert_eq!(restored_dt.hour() as u16, hour);
            prop_assert_eq!(restored_dt.minute() as u16, minute);
            prop_assert_eq!(restored_dt.second() as u16, second);
        }

        /// All valid DosDateTime values pass is_valid().
        #[test]
        fn valid_dos_datetime_proptest(
            year_offset in 0u16..=DosDateTime::MAX_YEAR_OFFSET,
            month in 1u16..=12,
            day in 1u16..=28,
            hour in 0u16..=23,
            minute in 0u16..=59,
            second_half in 0u16..=29,
        ) {
            let date = (year_offset << 9) | (month << 5) | day;
            let time = (hour << 11) | (minute << 5) | second_half;
            let dos = DosDateTime::new(date, time);

            prop_assert!(dos.is_valid());
        }

        /// Ordering matches chronological order.
        ///
        /// Verifies that `Ord` comparison produces the same result as comparing
        /// the individual date/time components in chronological order.
        ///
        /// This includes sorting of invalid date.
        #[test]
        fn ordering_matches_chronological(
            y1 in 0u16..=DosDateTime::MAX_YEAR_OFFSET,
            m1 in 1u16..=12,
            d1 in 1u16..=31,
            h1 in 0u16..=23,
            min1 in 0u16..=59,
            s1 in 0u16..=29,
            y2 in 0u16..=DosDateTime::MAX_YEAR_OFFSET,
            m2 in 1u16..=12,
            d2 in 1u16..=31,
            h2 in 0u16..=23,
            min2 in 0u16..=59,
            s2 in 0u16..=29,
        ) {
            let date1 = (y1 << 9) | (m1 << 5) | d1;
            let time1 = (h1 << 11) | (min1 << 5) | s1;
            let dos1 = DosDateTime::new(date1, time1);

            let date2 = (y2 << 9) | (m2 << 5) | d2;
            let time2 = (h2 << 11) | (min2 << 5) | s2;
            let dos2 = DosDateTime::new(date2, time2);

            // Compare chronologically using tuples.
            let chrono1 = (y1, m1, d1, h1, min1, s1);
            let chrono2 = (y2, m2, d2, h2, min2, s2);

            prop_assert_eq!(
                dos1.cmp(&dos2),
                chrono1.cmp(&chrono2),
                "Ord mismatch: {:?} vs {:?}",
                dos1,
                dos2
            );
        }

        /// Full range test including invalid calendar dates.
        ///
        /// Generates all possible date/time values (including invalid ones like Feb 31).
        /// Valid dates must produce valid DosDateTime; invalid dates are skipped.
        /// Years before 1980 clamp to 1980-01-01 00:00:00.
        /// Years after 2107 clamp to 2107-12-31 23:59:58.
        #[test]
        fn system_time_full_range_proptest(
            year in 1970i32..=2150,
            month in 1u32..=12,
            day in 1u32..=31,   // Includes invalid days like Feb 30
            hour in 0u32..=23,
            minute in 0u32..=59,
            second in 0u32..=59,
        ) {
            match Utc.with_ymd_and_hms(year, month, day, hour, minute, second).single() {
                Some(dt) => {
                    // Valid calendar date - conversion must produce valid DosDateTime.
                    let system_time = SystemTime::from(dt);
                    let dos = DosDateTime::from(system_time);

                    prop_assert!(dos.is_valid(),
                        "valid date {}-{:02}-{:02} {:02}:{:02}:{:02} produced invalid DosDateTime",
                        year, month, day, hour, minute, second);

                    // Verify clamping behavior.
                    if year < 1980 {
                        // Clamps to 1980-01-01 00:00:00.
                        prop_assert_eq!(dos.year(), 1980);
                        prop_assert_eq!(dos.month(), 1);
                        prop_assert_eq!(dos.day(), 1);
                        prop_assert_eq!(dos.hour(), 0);
                        prop_assert_eq!(dos.minute(), 0);
                        prop_assert_eq!(dos.second(), 0);
                    } else if year > 2107 {
                        // Clamps to 2107-12-31 23:59:58.
                        prop_assert_eq!(dos.year(), 2107);
                        prop_assert_eq!(dos.month(), 12);
                        prop_assert_eq!(dos.day(), 31);
                        prop_assert_eq!(dos.hour(), 23);
                        prop_assert_eq!(dos.minute(), 59);
                        prop_assert_eq!(dos.second(), 58);
                    } else {
                        // Within range: fields preserved.
                        prop_assert_eq!(dos.year() as i32, year);
                        prop_assert_eq!(dos.month() as u32, month);
                        prop_assert_eq!(dos.day() as u32, day);
                        prop_assert_eq!(dos.hour() as u32, hour);
                        prop_assert_eq!(dos.minute() as u32, minute);
                        // Seconds truncate to 2-second resolution.
                        prop_assert_eq!(dos.second() as u32, second - (second % 2));
                    }
                }
                None => {
                    // Invalid calendar date (e.g., Feb 30) - chrono rejects it.
                    // Verify from_components also rejects it.
                    if (1980..=2107).contains(&year) && month <= 12 && day <= 31 && hour <= 23 && minute <= 59 && second <= 59 {
                        let result = DosDateTime::from_components(
                            year as u16, month as u16, day as u16,
                            hour as u16, minute as u16, second as u16,
                        );

                        prop_assert!(result.is_err(),
                            "invalid date {}-{:02}-{:02} {:02}:{:02}:{:02} was accepted by from_components",
                            year, month, day, hour, minute, second);
                    }
                }
            }
        }
    }
}
