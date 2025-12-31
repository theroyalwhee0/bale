//! Extract command implementation.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use bale::ArchiveReader;

use crate::error::BaleCliError;

/// Extracts entries from an archive.
///
/// If no entries are specified, extracts all entries.
pub fn run(
    archive_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    entries: &[String],
) -> Result<(), BaleCliError> {
    let reader = ArchiveReader::open(archive_path)?;
    let output_dir = output_dir.as_ref();

    // Create output directory if it doesn't exist.
    fs::create_dir_all(output_dir)?;

    if entries.is_empty() {
        // Extract all entries.
        for (header, path_bytes) in reader.iter_entries() {
            extract_entry(&reader, header, path_bytes, output_dir)?;
        }
    } else {
        // Extract specified entries.
        for entry_path in entries {
            if let Some(header) = reader.find_entry(entry_path) {
                // Get path bytes from the header by re-iterating (find_entry only returns header).
                for (h, path_bytes) in reader.iter_entries() {
                    if std::ptr::eq(h, header) {
                        extract_entry(&reader, header, path_bytes, output_dir)?;
                        break;
                    }
                }
            } else {
                return Err(bale::BaleError::EntryNotFound(entry_path.clone()).into());
            }
        }
    }

    Ok(())
}

/// Extracts a single entry to the output directory.
fn extract_entry(
    reader: &ArchiveReader,
    header: &bale::CentralDirectoryHeader,
    path_bytes: &[u8],
    output_dir: &Path,
) -> Result<(), BaleCliError> {
    // Convert path bytes to string.
    let path: String = path_bytes
        .iter()
        .copied()
        .take_while(|&b| b != 0)
        .map(|b| b as char)
        .collect();

    let dest_path = output_dir.join(&path);

    // Create parent directories.
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read and write data.
    let data = reader.read_data(header)?;
    let mut file = File::create(&dest_path)?;
    file.write_all(data)?;

    // Set permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = header.external_attrs.get() >> 16;
        if mode != 0 {
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
        }
    }

    #[allow(clippy::print_stdout)]
    {
        println!("  extracted: {path}");
    }

    Ok(())
}
