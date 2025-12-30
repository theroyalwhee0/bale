use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DosDateTime {
    /// MS-DOS date field.
    pub date: u16,
    /// MS-DOS time field.
    pub time: u16,
}

impl DosDateTime {
    /// The MS-DOS epoch year.
    const DOS_EPOCH_YEAR: i32 = 1980;

    /// Creates a new `DosDateTime` from date and time fields.
    #[must_use]
    pub const fn new(date: u16, time: u16) -> Self {
        Self { date, time }
    }

    /// Extracts the year (1980-2107).
    #[must_use]
    pub const fn year(&self) -> u16 {
        ((self.date >> 9) & 0x7F) + Self::DOS_EPOCH_YEAR as u16
    }

    /// Extracts the month (1-12).
    #[must_use]
    pub const fn month(&self) -> u16 {
        (self.date >> 5) & 0x0F
    }

    /// Extracts the day (1-31).
    #[must_use]
    pub const fn day(&self) -> u16 {
        self.date & 0x1F
    }

    /// Extracts the hour (0-23).
    #[must_use]
    pub const fn hour(&self) -> u16 {
        (self.time >> 11) & 0x1F
    }

    /// Extracts the minute (0-59).
    #[must_use]
    pub const fn minute(&self) -> u16 {
        (self.time >> 5) & 0x3F
    }

    /// Extracts the second (0-58, always even).
    #[must_use]
    pub const fn second(&self) -> u16 {
        (self.time & 0x1F) * 2
    }
}

impl From<SystemTime> for DosDateTime {
    fn from(time: SystemTime) -> Self {
        let dt: DateTime<Utc> = time.into();

        let year = (dt.year() - Self::DOS_EPOCH_YEAR).clamp(0, 127) as u16;
        let month = dt.month() as u16;
        let day = dt.day() as u16;
        let hour = dt.hour() as u16;
        let minute = dt.minute() as u16;
        let second = dt.second() as u16;

        let date = (year << 9) | (month << 5) | day;
        let time = (hour << 11) | (minute << 5) | (second / 2);

        Self { date, time }
    }
}

impl From<DosDateTime> for SystemTime {
    fn from(dos: DosDateTime) -> Self {
        let dt = Utc
            .with_ymd_and_hms(
                dos.year() as i32,
                dos.month() as u32,
                dos.day() as u32,
                dos.hour() as u32,
                dos.minute() as u32,
                dos.second() as u32,
            )
            .single()
            .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());

        dt.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    /// SystemTime -> DosDateTime -> SystemTime preserves date/time components.
    #[test]
    fn roundtrip() {
        // 2024-06-15 13:10:42 UTC
        let original = Utc.with_ymd_and_hms(2024, 6, 15, 13, 10, 42).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        let restored: SystemTime = dos.into();
        let restored_dt: DateTime<Utc> = restored.into();

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
    }

    /// Dates before DOS epoch (1980) clamp to 1980.
    #[test]
    fn before_dos_epoch() {
        // 1970-01-01 should clamp to 1980-01-01
        let original = Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap();
        let dos = DosDateTime::from(SystemTime::from(original));
        assert_eq!(dos.year(), 1980);
    }
}
