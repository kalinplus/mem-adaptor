//! Loads a complete local source inventory before Readers or target writes run.
//! ZIP names are preflighted before extraction; payload I/O may still fail afterwards.
//! This is not a resource quota, race-free filesystem snapshot, or recovery mechanism.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path};

use anyhow::{Context, bail, ensure};

use crate::Result;
use crate::plugins::{FileInventory, SourceFs};

/// Loads every supported source file or returns the ordinary I/O/ZIP error without skipping failures.
/// ZIP extraction uses an automatically removed temporary directory; the original root remains the locator.
pub fn load_source(path: &Path) -> Result<SourceFs> {
    let root = fs::canonicalize(path).with_context(|| {
        format!(
            "Cannot open source: {}",
            crate::gate::mask(&path.to_string_lossy())
        )
    })?;
    let temporary;
    let directory = if root.is_dir() {
        root.as_path()
    } else {
        ensure!(
            root.is_file()
                && root
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("zip")),
            "Source must be a directory or ZIP archive"
        );
        temporary = tempfile::tempdir()?;
        extract_zip(&root, temporary.path())?;
        temporary.path()
    };
    let mut files = BTreeMap::new();
    visit(directory, directory, &mut files)?;
    Ok(SourceFs { root, files })
}

/// Recursively inventories real files, rejecting child symlinks and propagating any failed read.
fn visit(root: &Path, directory: &Path, files: &mut FileInventory) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "Source symlinks are not supported");
        if kind.is_dir() {
            visit(root, &entry.path(), files)?;
        } else if kind.is_file() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .context("Source path is not UTF-8")?
                .to_owned();
            ensure!(
                files.insert(relative, fs::read(&path)?).is_none(),
                "Duplicate source path"
            );
        } else {
            bail!("Unsupported source file type");
        }
    }
    Ok(())
}

/// Checks raw central names because zip 8.6 folds duplicate names into its public index.
/// The dependency validates ZIP/ZIP64 framing; this raw header walk only checks names before extraction.
fn validate_zip_names(file: &mut fs::File, central_start: u64) -> Result<()> {
    file.seek(SeekFrom::Start(central_start))?;
    let mut names = std::collections::BTreeSet::new();
    loop {
        let mut signature = [0; 4];
        file.read_exact(&mut signature)?;
        if signature != *b"PK\x01\x02" {
            return Ok(());
        }
        let mut header = [0; 42];
        file.read_exact(&mut header)?;
        let name_len = u16::from_le_bytes([header[24], header[25]]);
        let extra_len = u16::from_le_bytes([header[26], header[27]]);
        let comment_len = u16::from_le_bytes([header[28], header[29]]);
        let mut name = vec![0; usize::from(name_len)];
        file.read_exact(&mut name)?;
        ensure!(names.insert(name), "Duplicate ZIP path");
        file.seek(SeekFrom::Current(
            i64::from(extra_len) + i64::from(comment_len),
        ))?;
    }
}

/// Rejects unsafe or duplicate names before any extraction; later payload errors may leave temporary files.
fn extract_zip(path: &Path, destination: &Path) -> Result<()> {
    let mut file = fs::File::open(path)?;
    let mut archive =
        zip::ZipArchive::new(file.try_clone()?).context("Cannot parse ZIP archive")?;
    validate_zip_names(&mut file, archive.central_directory_start())?;
    let mut names = std::collections::BTreeSet::new();
    // Validate the entire archive before creating any extracted file.
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name();
        ensure!(
            !name.contains('\\') && !name.contains('\0'),
            "Unsafe ZIP path"
        );
        let enclosed = Path::new(name);
        ensure!(
            !enclosed.is_absolute()
                && !enclosed
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_))),
            "Unsafe ZIP path"
        );
        ensure!(
            !name.as_bytes().get(1).is_some_and(|byte| *byte == b':'),
            "Unsafe ZIP drive path"
        );
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "ZIP symlinks are not supported"
        );
        let normalized: std::path::PathBuf = enclosed
            .components()
            .filter(|part| !matches!(part, Component::CurDir))
            .collect();
        ensure!(names.insert(normalized), "Duplicate ZIP path");
    }
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let output = destination.join(entry.name());
        if entry.is_dir() {
            fs::create_dir_all(output)?;
        } else {
            fs::create_dir_all(output.parent().unwrap())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            let mut buffer = [0u8; 65536];
            loop {
                let count = entry.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                file.write_all(&buffer[..count])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates one synthetic archive entry, optionally a symlink, without accessing external data.
    fn archive(directory: &Path, name: &str, symlink: bool) -> std::path::PathBuf {
        let path = directory.join("synthetic.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        if symlink {
            zip.add_symlink(name, "../outside", options).unwrap();
        } else {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"Synthetic memory").unwrap();
        }
        zip.finish().unwrap();
        path
    }

    /// Rejects traversal, drive paths, backslashes and links without creating an outside artifact.
    #[test]
    fn malicious_zip_paths_and_symlinks_are_rejected() {
        for (name, symlink, cause) in [
            ("../outside.md", false, "Unsafe ZIP path"),
            ("/absolute.md", false, "Unsafe ZIP path"),
            ("C:/outside.md", false, "Unsafe ZIP drive path"),
            ("folder\\outside.md", false, "Unsafe ZIP path"),
            ("link.md", true, "ZIP symlinks are not supported"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let zip = archive(directory.path(), name, symlink);
            let error = load_source(&zip).err().unwrap();
            assert_eq!(error.to_string(), cause);
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    /// Retains nested payload bytes and the original archive locator, not the disposable extraction root.
    #[test]
    fn safe_zip_is_read_with_original_source_locator() {
        let directory = tempfile::tempdir().unwrap();
        let zip = archive(directory.path(), "nested/example.md", false);
        let source = load_source(&zip).unwrap();
        assert_eq!(source.root, fs::canonicalize(zip).unwrap());
        assert_eq!(source.files["nested/example.md"], b"Synthetic memory");
    }

    /// Keeps the extraction directory empty when a dangerous entry follows a safe entry.
    #[test]
    fn full_archive_is_validated_before_any_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.zip");
        let destination = directory.path().join("extracted");
        fs::create_dir(&destination).unwrap();
        fs::write(
            directory.path().join("outside.md"),
            b"Protected outside bytes",
        )
        .unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["safe.md", "../outside.md"] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"Synthetic memory").unwrap();
        }
        zip.finish().unwrap();
        let error = extract_zip(&path, &destination).unwrap_err();
        assert_eq!(error.to_string(), "Unsafe ZIP path");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert_eq!(
            fs::read(directory.path().join("outside.md")).unwrap(),
            b"Protected outside bytes"
        );
    }

    /// Patches equal-length local and central names because ZipWriter itself disallows duplicate names.
    fn duplicate_archive(directory: &Path) -> std::path::PathBuf {
        let path = directory.join("duplicate.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for name in ["first.md", "other.md"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"Synthetic").unwrap();
        }
        zip.finish().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let mut replacements = 0;
        for index in 0..=bytes.len() - 8 {
            if &bytes[index..index + 8] == b"other.md" {
                bytes[index..index + 8].copy_from_slice(b"first.md");
                replacements += 1;
            }
        }
        assert_eq!(replacements, 2);
        fs::write(&path, bytes).unwrap();
        path
    }

    /// Rejects duplicate raw central names even when the dependency silently indexes only the last entry.
    #[test]
    fn duplicate_zip_names_fail_before_any_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let path = duplicate_archive(directory.path());
        let destination = directory.path().join("extracted");
        fs::create_dir(&destination).unwrap();
        let error = extract_zip(&path, &destination).unwrap_err();
        assert_eq!(error.to_string(), "Duplicate ZIP path");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    /// Treats dot-segment aliases as duplicate extraction paths rather than failing after the first file.
    #[test]
    fn normalized_zip_aliases_fail_before_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("aliases.zip");
        let destination = directory.path().join("extracted");
        fs::create_dir(&destination).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for name in ["first.md", "./first.md"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"Synthetic").unwrap();
        }
        zip.finish().unwrap();
        let error = extract_zip(&path, &destination).unwrap_err();
        assert_eq!(error.to_string(), "Duplicate ZIP path");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    /// Distinguishes malformed ZIP input from a missing source's underlying ordinary I/O error.
    #[test]
    fn corrupt_zip_and_missing_source_keep_error_categories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("corrupt.zip");
        fs::write(&path, b"not an archive").unwrap();
        let error = load_source(&path).err().unwrap();
        assert!(matches!(
            error.downcast_ref::<zip::result::ZipError>(),
            Some(zip::result::ZipError::InvalidArchive(_))
        ));
        let token = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
        let error = load_source(&directory.path().join(format!("missing-{token},")))
            .err()
            .unwrap();
        assert!(!format!("{error:#}").contains(&token));
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    /// Does not follow a local child symlink or alter the file outside the source.
    #[cfg(unix)]
    #[test]
    fn local_source_symlink_is_rejected_without_following_it() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let outside = directory.path().join("outside.md");
        fs::create_dir(&source).unwrap();
        fs::write(&outside, b"Protected synthetic bytes").unwrap();
        std::os::unix::fs::symlink(&outside, source.join("link.md")).unwrap();
        let error = load_source(&source).err().unwrap();
        assert_eq!(error.to_string(), "Source symlinks are not supported");
        assert_eq!(fs::read(outside).unwrap(), b"Protected synthetic bytes");
    }

    /// Keeps unsupported compression and broken payload integrity as ZIP failures, not partial inventories.
    #[test]
    fn unsupported_compression_and_crc_errors_propagate() {
        for unsupported in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let path = archive(directory.path(), "memory.md", false);
            let mut bytes = fs::read(&path).unwrap();
            let central = bytes
                .windows(4)
                .position(|part| part == b"PK\x01\x02")
                .unwrap();
            if unsupported {
                // Compression method is repeated in the local and central headers.
                bytes[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
                bytes[central + 10..central + 12].copy_from_slice(&u16::MAX.to_le_bytes());
            } else {
                bytes[central + 16] ^= 1; // Central CRC is authoritative for the reader.
            }
            fs::write(&path, bytes).unwrap();
            let error = load_source(&path).err().unwrap();
            if unsupported {
                assert!(matches!(
                    error.downcast_ref::<zip::result::ZipError>().unwrap(),
                    zip::result::ZipError::CompressionMethodNotSupported(65535)
                ));
            } else {
                assert_eq!(
                    error.downcast_ref::<std::io::Error>().unwrap().kind(),
                    std::io::ErrorKind::InvalidData
                );
            }
            // Extraction payload errors need not leave the temporary directory empty, but it is removed.
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    /// Preserves supported ZIP64 local entries and comments when walking variable central-header fields.
    #[test]
    fn zip64_entry_and_archive_comment_remain_readable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.set_comment("Synthetic archive PK\x01\x02 comment")
            .unwrap();
        zip.start_file(
            "nested/memory.md",
            zip::write::SimpleFileOptions::default().large_file(true),
        )
        .unwrap();
        zip.write_all(b"Synthetic memory").unwrap();
        zip.finish().unwrap();
        let source = load_source(&path).unwrap();
        assert_eq!(source.files["nested/memory.md"], b"Synthetic memory");
    }
}
