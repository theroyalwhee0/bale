//! Options for adding entries to an archive.

/// Options for adding an entry to an archive.
///
/// Controls timestamp behavior when adding entries. By default (all fields
/// `None`), timestamps are set to the current time — matching the behavior
/// of [`add_entry`](super::ArchiveWrite::add_entry).
///
/// Set `created_time` and/or `modified_time` to `Some(millis)` to override
/// with specific Unix epoch millisecond values. This is useful for
/// compaction and other operations that need to preserve original timestamps.
#[derive(Debug, Clone, Default)]
pub struct AddEntryOptions {
    /// Creation timestamp as Unix epoch milliseconds.
    ///
    /// When `None`, defaults to the current time.
    pub created_time: Option<i64>,

    /// Modification timestamp as Unix epoch milliseconds.
    ///
    /// When `None`, defaults to the current time.
    pub modified_time: Option<i64>,
}
