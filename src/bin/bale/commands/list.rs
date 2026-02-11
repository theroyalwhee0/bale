//! List command implementation.

use std::path::Path;

use bale::{ArchivePath, ArchiveRead, ArchiveReader};
use nix::sys::stat::{Mode, SFlag};

use crate::error::BaleCliError;

/// Month abbreviations for date formatting.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Executable permission mask (any of user/group/other execute).
const EXEC_MASK: u32 = Mode::S_IXUSR.bits() | Mode::S_IXGRP.bits() | Mode::S_IXOTH.bits();

/// Lists entries in an archive with ls -alF style output.
///
/// Output format: `<permissions> <size> <date> <time> <path><indicator>`
///
/// Example: `-rw-r--r--      1234 Jan  2 18:30 hello.txt`
pub fn run(archive_path: impl AsRef<Path>) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;

    for (entry_row, path_bytes) in reader.iter_entries() {
        let path = ArchivePath::from_null_padded_bytes(path_bytes);
        let size = entry_row.file_size.get();
        let mode = entry_row.mode.get();
        let mtime_ms = entry_row.modified_time.get();

        let perms = format_permissions(mode);
        let date_time = format_mtime_millis(mtime_ms);
        let indicator = get_type_indicator(mode, path.as_str().unwrap_or(""));

        #[allow(clippy::print_stdout)]
        {
            println!("{perms} {size:>10} {date_time} {path}{indicator}");
        }
    }

    Ok(())
}

/// Formats Unix mode bits as a permission string (e.g., `-rw-r--r--`).
fn format_permissions(mode: u32) -> String {
    let file_type = match mode & SFlag::S_IFMT.bits() {
        x if x == SFlag::S_IFDIR.bits() => 'd',
        x if x == SFlag::S_IFLNK.bits() => 'l',
        x if x == SFlag::S_IFREG.bits() => '-',
        _ => '-', // Default to regular file
    };

    let perms = [
        if mode & Mode::S_IRUSR.bits() != 0 {
            'r'
        } else {
            '-'
        },
        if mode & Mode::S_IWUSR.bits() != 0 {
            'w'
        } else {
            '-'
        },
        if mode & Mode::S_IXUSR.bits() != 0 {
            'x'
        } else {
            '-'
        },
        if mode & Mode::S_IRGRP.bits() != 0 {
            'r'
        } else {
            '-'
        },
        if mode & Mode::S_IWGRP.bits() != 0 {
            'w'
        } else {
            '-'
        },
        if mode & Mode::S_IXGRP.bits() != 0 {
            'x'
        } else {
            '-'
        },
        if mode & Mode::S_IROTH.bits() != 0 {
            'r'
        } else {
            '-'
        },
        if mode & Mode::S_IWOTH.bits() != 0 {
            'w'
        } else {
            '-'
        },
        if mode & Mode::S_IXOTH.bits() != 0 {
            'x'
        } else {
            '-'
        },
    ];

    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        file_type,
        perms[0],
        perms[1],
        perms[2],
        perms[3],
        perms[4],
        perms[5],
        perms[6],
        perms[7],
        perms[8]
    )
}

/// Formats a Unix epoch millisecond timestamp as `Mon DD HH:MM`.
fn format_mtime_millis(mtime_ms: i64) -> String {
    let dt =
        chrono::DateTime::from_timestamp_millis(mtime_ms).unwrap_or(chrono::DateTime::UNIX_EPOCH);
    let month_idx = (chrono::Datelike::month(&dt) as usize).saturating_sub(1);
    let month = MONTHS.get(month_idx).unwrap_or(&"???");
    let day = chrono::Datelike::day(&dt);
    let hour = chrono::Timelike::hour(&dt);
    let minute = chrono::Timelike::minute(&dt);

    format!("{month} {day:2} {hour:02}:{minute:02}")
}

/// Returns the type indicator character for ls -F style output.
///
/// - `/` for directories (unless path already ends with `/`)
/// - `*` for executable files
/// - `@` for symlinks
/// - empty for regular files
fn get_type_indicator(mode: u32, path: &str) -> &'static str {
    let file_type = mode & SFlag::S_IFMT.bits();

    // Don't add indicator if path already has a trailing slash.
    if path.ends_with('/') {
        return "";
    }

    match file_type {
        x if x == SFlag::S_IFDIR.bits() => "/",
        x if x == SFlag::S_IFLNK.bits() => "@",
        _ if mode & EXEC_MASK != 0 => "*", // Executable
        _ => "",                           // Regular file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regular file permissions format correctly.
    #[test]
    fn format_regular_file_permissions() {
        assert_eq!(format_permissions(0o100644), "-rw-r--r--");
        assert_eq!(format_permissions(0o100755), "-rwxr-xr-x");
        assert_eq!(format_permissions(0o100600), "-rw-------");
    }

    /// Directory permissions format correctly.
    #[test]
    fn format_directory_permissions() {
        assert_eq!(format_permissions(0o040755), "drwxr-xr-x");
        assert_eq!(format_permissions(0o040700), "drwx------");
    }

    /// Symlink permissions format correctly.
    #[test]
    fn format_symlink_permissions() {
        assert_eq!(format_permissions(0o120777), "lrwxrwxrwx");
    }

    /// Date formatting works correctly with epoch milliseconds.
    #[test]
    fn format_date() {
        // 2024-01-02 18:30:00 UTC in milliseconds.
        let mtime_ms = 1_704_220_200_000i64;
        let result = format_mtime_millis(mtime_ms);
        // Verify format is "Mon DD HH:MM".
        assert_eq!(result.len(), 12);
        assert!(result.contains(':'));
    }

    /// Type indicators are correct.
    #[test]
    fn type_indicators() {
        assert_eq!(get_type_indicator(0o040755, "dir"), "/");
        assert_eq!(get_type_indicator(0o040755, "dir/"), ""); // Already has slash
        assert_eq!(get_type_indicator(0o120777, "link"), "@");
        assert_eq!(get_type_indicator(0o100755, "script"), "*");
        assert_eq!(get_type_indicator(0o100644, "file.txt"), "");
    }
}
