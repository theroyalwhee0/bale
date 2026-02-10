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
    pub(crate) const MAX_YEAR_OFFSET: u16 = 127;

    // Bit masks for extracting date/time components (applied after shifting).
    /// Mask for year offset (7 bits).
    const YEAR_MASK: u16 = 0x7F;
    /// Mask for month (4 bits).
    const MONTH_MASK: u16 = 0x0F;
    /// Mask for day (5 bits).
    const DAY_MASK: u16 = 0x1F;
    /// Mask for hour (5 bits).
    const HOUR_MASK: u16 = 0x1F;
    /// Mask for minute (6 bits).
    const MINUTE_MASK: u16 = 0x3F;
    /// Mask for seconds/2 (5 bits).
    const SECOND_MASK: u16 = 0x1F;

    // DOS epoch encoded values.
    /// DOS epoch date: 1980-01-01 encoded as (0 << 9) | (1 << 5) | 1.
    const DOS_EPOCH_DATE: u16 = 0x0021;
    /// Midnight time: 00:00:00 encoded as 0.
    const MIDNIGHT: u16 = 0x0000;

    /// Creates a `DosDateTime` from raw date and time fields.
    ///
    /// The `date` and `time` parameters are the packed MS-DOS format values
    /// as stored in archive headers. For human-readable components, use
    /// [`from_components`](Self::from_components).
    #[must_use]
    pub const fn from_date_time_parts(date: u16, time: u16) -> Self {
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
            && Self::days_in_month(year, month).is_some_and(|max| day <= max)
            && hour <= 23
            && minute <= 59
            && second <= 59; // Accepts 0-59; truncated to 0-29 (even seconds 0-58) below.

        if !valid {
            return Err(BaleError::InvalidDosDateTime(format!(
                "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
            )));
        }

        // Safe to calculate now - all components are validated.
        let year_offset = year - Self::DOS_EPOCH_YEAR;
        let date = (year_offset << 9) | (month << 5) | day;
        // Truncate seconds to 2-second resolution: 59 -> 29, which displays as 58.
        let time = (hour << 11) | (minute << 5) | (second / 2);

        Ok(Self { date, time })
    }

    /// Returns the current time as a `DosDateTime`.
    ///
    /// This is equivalent to `DosDateTime::from(SystemTime::now())`.
    ///
    /// # Example
    ///
    /// ```
    /// use bale::DosDateTime;
    ///
    /// let now = DosDateTime::now();
    /// assert!(now.is_valid());
    /// ```
    #[must_use]
    pub fn now() -> Self {
        Self::from(SystemTime::now())
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
    ///
    /// Returns `None` for invalid months (0 or > 12).
    pub(crate) const fn days_in_month(year: u16, month: u16) -> Option<u16> {
        match month {
            // Months with 31 days.
            1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
            // Months with 30 days.
            4 | 6 | 9 | 11 => Some(30),
            // February.
            2 => {
                // Leap year: divisible by 4, except centuries unless divisible by 400.
                let is_leap = year.is_multiple_of(4)
                    && (!year.is_multiple_of(100) || year.is_multiple_of(400));
                if is_leap { Some(29) } else { Some(28) }
            }
            // Invalid month.
            _ => None,
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
        let max_day = match Self::days_in_month(self.year(), month) {
            Some(d) => d,
            None => return false, // Invalid month already handled, but be defensive.
        };
        if day < 1 || day > max_day {
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
            // Note: 58 is the max representable second (stored as 29 * 2), not 59 truncated.
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
        .ok_or_else(|| BaleError::InvalidDosDateTime(dos.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    use crate::proptest_config;

    // ==================== Unit Tests ====================

    /// The DOS epoch (1980-01-01 00:00:00) encodes correctly.
    #[test]
    fn dos_epoch() {
        let dos = DosDateTime::from_date_time_parts(0x0021, 0x0000);
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
        assert!(dos.is_valid());
    }

    /// Feb 29 is valid in leap years (divisible by 4).
    #[test]
    fn feb_29_valid_in_leap_year() {
        // 2024 is a leap year (divisible by 4)
        let dos = DosDateTime::from_components(2024, 2, 29, 12, 0, 0).unwrap();
        assert_eq!(dos.month(), 2);
        assert_eq!(dos.day(), 29);
        assert!(dos.is_valid());
    }

    /// Feb 29 is valid in century leap years (divisible by 400).
    #[test]
    fn feb_29_valid_in_century_leap_year() {
        // 2000 is a leap year (divisible by 400)
        let dos = DosDateTime::from_components(2000, 2, 29, 12, 0, 0).unwrap();
        assert_eq!(dos.month(), 2);
        assert_eq!(dos.day(), 29);
        assert!(dos.is_valid());
    }

    /// Feb 29 is invalid in non-leap years.
    #[test]
    fn feb_29_invalid_in_non_leap_year() {
        // 2023 is not a leap year
        assert!(DosDateTime::from_components(2023, 2, 29, 12, 0, 0).is_err());
    }

    /// Feb 29 is invalid in century non-leap years (divisible by 100 but not 400).
    #[test]
    fn feb_29_invalid_in_century_non_leap_year() {
        // 2100 is not a leap year (divisible by 100 but not 400)
        assert!(DosDateTime::from_components(2100, 2, 29, 12, 0, 0).is_err());
    }

    /// `DosDateTime` can be used in hash collections.
    #[test]
    fn hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DosDateTime::from_date_time_parts(0x0021, 0x0000));
        set.insert(DosDateTime::from_date_time_parts(0x0021, 0x0000));
        set.insert(DosDateTime::from_date_time_parts(0x5921, 0x6B15));
        assert_eq!(set.len(), 2);
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
        let dos = DosDateTime::from_date_time_parts(0x005E, 0x0000); // Feb 30
        assert!(SystemTime::try_from(dos).is_err());
    }

    /// `to_system_time_or_epoch()` falls back to DOS epoch for invalid dates.
    #[test]
    fn to_system_time_or_epoch_falls_back() {
        let dos = DosDateTime::from_date_time_parts(0x005E, 0x0000); // Feb 30
        let dt: DateTime<Utc> = dos.to_system_time_or_epoch().into();
        assert_eq!((dt.year(), dt.month(), dt.day()), (1980, 1, 1));
    }

    // ==================== Property Tests ====================

    proptest! {
        #![proptest_config(proptest_config::config())]

        /// Valid DosDateTime values from components pass is_valid().
        #[test]
        fn valid_components_accepted(
            year in 1980u16..=2107,
            month in 1u16..=12,
            day in 1u16..=28, // Safe range for all months
            hour in 0u16..=23,
            minute in 0u16..=59,
            second in 0u16..=59,
        ) {
            let dos = DosDateTime::from_components(year, month, day, hour, minute, second)
                .expect("valid components");
            prop_assert!(dos.is_valid());
            prop_assert_eq!(dos.year(), year);
            prop_assert_eq!(dos.month(), month);
            prop_assert_eq!(dos.day(), day);
            prop_assert_eq!(dos.hour(), hour);
            prop_assert_eq!(dos.minute(), minute);
            prop_assert_eq!(dos.second(), second / 2 * 2); // 2-second resolution
        }

        /// Invalid months (0 or 13+) fail is_valid().
        #[test]
        fn invalid_month_rejected(month in prop_oneof![Just(0u16), 13u16..=15]) {
            let date = (month << 5) | 1; // year_offset=0, day=1
            let dos = DosDateTime::from_date_time_parts(date, 0x0000);
            prop_assert!(!dos.is_valid());
        }

        /// Day 0 fails is_valid().
        #[test]
        fn day_zero_rejected(month in 1u16..=12) {
            let date = month << 5; // year_offset=0, day=0
            let dos = DosDateTime::from_date_time_parts(date, 0x0000);
            prop_assert!(!dos.is_valid());
        }

        /// Days beyond month's max fail is_valid().
        #[test]
        fn excess_day_rejected(
            year_offset in 0u16..=127,
            month in 1u16..=12,
        ) {
            let year = 1980 + year_offset;
            let max_day = DosDateTime::days_in_month(year, month).unwrap();
            let invalid_day = max_day + 1;
            let date = (year_offset << 9) | (month << 5) | invalid_day;
            let dos = DosDateTime::from_date_time_parts(date, 0x0000);
            prop_assert!(!dos.is_valid(), "day {} in month {} should be invalid", invalid_day, month);
        }

        /// Invalid hours (24+) fail is_valid().
        #[test]
        fn invalid_hour_rejected(hour in 24u16..=31) {
            let time = hour << 11;
            let dos = DosDateTime::from_date_time_parts(0x0021, time);
            prop_assert!(!dos.is_valid());
        }

        /// Invalid minutes (60+) fail is_valid().
        #[test]
        fn invalid_minute_rejected(minute in 60u16..=63) {
            let time = minute << 5;
            let dos = DosDateTime::from_date_time_parts(0x0021, time);
            prop_assert!(!dos.is_valid());
        }

        /// Invalid seconds (60+) fail is_valid().
        #[test]
        fn invalid_second_rejected(second_half in 30u16..=31) {
            let dos = DosDateTime::from_date_time_parts(0x0021, second_half);
            prop_assert!(!dos.is_valid());
        }

        /// days_in_month returns None for invalid months.
        #[test]
        fn days_in_month_invalid(month in prop_oneof![Just(0u16), 13u16..=255]) {
            prop_assert_eq!(DosDateTime::days_in_month(2000, month), None);
        }

        /// days_in_month returns correct values for valid months.
        #[test]
        fn days_in_month_valid(
            year in 1980u16..=2107,
            month in 1u16..=12,
        ) {
            let expected = match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 => {
                    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
                    if leap { 29 } else { 28 }
                }
                _ => unreachable!(),
            };
            prop_assert_eq!(DosDateTime::days_in_month(year, month), Some(expected));
        }

        /// Odd seconds truncate to even (2-second resolution).
        #[test]
        fn odd_seconds_truncated(second in 0u16..=59) {
            let dos = DosDateTime::from_components(2000, 1, 1, 0, 0, second).unwrap();
            prop_assert_eq!(dos.second(), second / 2 * 2);
        }

        /// from_components rejects year before epoch.
        #[test]
        fn year_before_epoch_rejected(year in 0u16..1980) {
            prop_assert!(DosDateTime::from_components(year, 1, 1, 0, 0, 0).is_err());
        }

        /// from_components rejects year after max.
        #[test]
        fn year_after_max_rejected(year in 2108u16..=u16::MAX) {
            prop_assert!(DosDateTime::from_components(year, 1, 1, 0, 0, 0).is_err());
        }

        /// Dates before DOS epoch clamp to 1980-01-01 00:00:00.
        #[test]
        fn before_epoch_clamps(year in 1970i32..1980) {
            let dt = Utc.with_ymd_and_hms(year, 6, 15, 12, 30, 0).unwrap();
            let dos = DosDateTime::from(SystemTime::from(dt));
            prop_assert_eq!(dos.year(), 1980);
            prop_assert_eq!(dos.month(), 1);
            prop_assert_eq!(dos.day(), 1);
            prop_assert!(dos.is_valid());
        }

        /// Dates after max DOS year clamp to 2107-12-31 23:59:58.
        #[test]
        fn after_max_clamps(year in 2108i32..=2150) {
            let dt = Utc.with_ymd_and_hms(year, 6, 15, 12, 30, 0).unwrap();
            let dos = DosDateTime::from(SystemTime::from(dt));
            prop_assert_eq!(dos.year(), 2107);
            prop_assert_eq!(dos.month(), 12);
            prop_assert_eq!(dos.day(), 31);
            prop_assert!(dos.is_valid());
        }

        /// Round-trip: SystemTime -> DosDateTime -> SystemTime preserves components.
        #[test]
        fn roundtrip(
            year in 1980u16..=2107,
            month in 1u16..=12,
            day in 1u16..=28,
            hour in 0u16..=23,
            minute in 0u16..=59,
            second in (0u16..=29).prop_map(|s| s * 2),
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

        /// Ordering matches chronological order.
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
            let dos1 = DosDateTime::from_date_time_parts((y1 << 9) | (m1 << 5) | d1, (h1 << 11) | (min1 << 5) | s1);
            let dos2 = DosDateTime::from_date_time_parts((y2 << 9) | (m2 << 5) | d2, (h2 << 11) | (min2 << 5) | s2);
            let chrono1 = (y1, m1, d1, h1, min1, s1);
            let chrono2 = (y2, m2, d2, h2, min2, s2);
            prop_assert_eq!(dos1.cmp(&dos2), chrono1.cmp(&chrono2));
        }
    }
}
