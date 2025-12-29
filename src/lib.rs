use std::fs::{FileTimes, OpenOptions};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Creates a file if it doesn't exist, or updates its modification time if it does.
pub fn touch(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;

    let now = SystemTime::now();
    let times = FileTimes::new().set_accessed(now).set_modified(now);
    file.set_times(times)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_workz() {}
}
