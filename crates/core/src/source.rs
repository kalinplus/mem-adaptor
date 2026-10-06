use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path};

use anyhow::{Context, bail, ensure};

use crate::Result;
use crate::plugins::{FileInventory, SourceFs};

pub fn load_source(path: &Path) -> Result<SourceFs> {
    let root = fs::canonicalize(path)
        .with_context(|| format!("Cannot open source: {}", path.display()))?;
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

fn extract_zip(path: &Path, destination: &Path) -> Result<()> {
    let mut archive =
        zip::ZipArchive::new(fs::File::open(path)?).context("Cannot parse ZIP archive")?;
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
        ensure!(names.insert(name.to_owned()), "Duplicate ZIP path");
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

    #[test]
    fn malicious_zip_paths_and_symlinks_are_rejected() {
        for (name, symlink) in [
            ("../outside.md", false),
            ("/absolute.md", false),
            ("C:/outside.md", false),
            ("folder\\outside.md", false),
            ("link.md", true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let zip = archive(directory.path(), name, symlink);
            assert!(load_source(&zip).is_err(), "{name}");
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn safe_zip_is_read_with_original_source_locator() {
        let directory = tempfile::tempdir().unwrap();
        let zip = archive(directory.path(), "nested/example.md", false);
        let source = load_source(&zip).unwrap();
        assert_eq!(source.root, fs::canonicalize(zip).unwrap());
        assert_eq!(source.files["nested/example.md"], b"Synthetic memory");
    }

    #[test]
    fn full_archive_is_validated_before_any_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic.zip");
        let destination = directory.path().join("extracted");
        fs::create_dir(&destination).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for name in ["safe.md", "../outside.md"] {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"Synthetic memory").unwrap();
        }
        zip.finish().unwrap();
        assert!(extract_zip(&path, &destination).is_err());
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        assert!(!directory.path().join("outside.md").exists());
    }
}
