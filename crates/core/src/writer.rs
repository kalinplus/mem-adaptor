//! Shared target-file boundaries used by local Writers and engine output verification.
//! Roots are resolved once before approval; later operations reject links and nonregular artifacts.
//! Byte checks and per-file replacements do not provide directory-handle isolation or a whole-run transaction.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::canonical::CanonicalRecord;
use crate::engine::content_hash;
use crate::reports::*;
use anyhow::ensure;

/// Lists canonical fields from the hand-authored schema for adapter capabilities.
pub fn fields() -> Vec<String> {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schema/canonical-record.schema.json")).unwrap();
    schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(|key| format!("/{key}"))
        .collect()
}

/// Describes a target projection without embedding source values in reports.
pub fn mapping(canonical: &str, target: &str, rule: &str) -> TargetMapping {
    TargetMapping {
        canonical_path: canonical.into(),
        target_path: target.into(),
        rule: rule.into(),
    }
}

/// Derives a bounded display title while preserving the record's original body.
pub fn title(record: &CanonicalRecord) -> String {
    record
        .content
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            line.trim()
                .trim_start_matches('#')
                .trim()
                .chars()
                .take(80)
                .collect()
        })
        .filter(|title: &String| !title.is_empty())
        .unwrap_or_else(|| format!("Memory {}", record.canonical_id))
}

/// Pins an existing physical prefix and missing suffix, rejecting an explicitly linked target root.
/// Existing ancestor aliases (including OS temporary-directory aliases) resolve before approval;
/// callers retain this result rather than resolving it again after a directory changes.
pub fn normalize_root(path: &Path) -> crate::Result<PathBuf> {
    let absolute: PathBuf = std::path::absolute(path)?.components().collect();
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Target root must be a regular directory, not a symlink"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir() || metadata.file_type().is_symlink(),
                    "Target ancestor must be a directory"
                );
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(
                    existing
                        .file_name()
                        .ok_or_else(|| anyhow::anyhow!("Invalid target root"))?
                        .to_owned(),
                );
                existing = existing
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Invalid target root"))?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut root = fs::canonicalize(existing)?;
    ensure!(root.is_dir(), "Target ancestor must be a directory");
    for component in suffix.into_iter().rev() {
        root.push(component);
    }
    Ok(root)
}

/// Checks every component of an already normalized directory without following new links.
/// Only absence returns false; links, non-directories and ordinary I/O failures refuse the operation.
pub fn directory_exists(path: &Path) -> crate::Result<bool> {
    ensure!(path.is_absolute(), "Target directory must be absolute");
    let mut current = PathBuf::new();
    for component in path.components() {
        ensure!(
            !matches!(component, Component::ParentDir | Component::CurDir),
            "Target directory must be normalized"
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Target directory or ancestor must be a regular directory, not a symlink"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

/// Reads only regular artifacts beneath the fixed root; only NotFound means no artifact.
/// Intermediate directories and the root's ancestors are rechecked, including after approval.
pub fn read_file(root: &Path, relative: &str) -> crate::Result<Option<Vec<u8>>> {
    let path = Path::new(relative);
    ensure!(
        !path.as_os_str().is_empty()
            && !relative.contains('\\')
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Invalid target artifact path"
    );
    if !directory_exists(root)? {
        return Ok(None);
    }
    let mut current = root.to_owned();
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Target artifact must not be a symlink"
                );
                ensure!(
                    if components.peek().is_some() {
                        metadata.is_dir()
                    } else {
                        metadata.is_file()
                    },
                    "Target artifact must be a regular file with directory ancestors"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(fs::read(current)?))
}

/// Observes artifact bytes for approval or proof checking without treating read errors as absence.
pub fn artifact(root: &Path, path: &str) -> crate::Result<TargetArtifact> {
    let bytes = read_file(root, path)?;
    Ok(TargetArtifact {
        path: path.into(),
        content_hash: bytes.as_ref().map(|bytes| content_hash(bytes)),
        bytes: bytes.as_ref().map(|bytes| bytes.len() as u64),
    })
}

/// Attests the exact bytes constructed by a Writer, not a later sample of edited output.
pub fn output_artifact(path: &str, bytes: &[u8]) -> TargetArtifact {
    TargetArtifact {
        path: path.into(),
        content_hash: Some(content_hash(bytes)),
        bytes: Some(bytes.len() as u64),
    }
}

/// Rechecks the expected bytes and directory chain around one temporary-file replacement.
/// Failure can follow directory creation; no lock prevents a concurrent swap after the last check.
pub fn atomic_file(
    root: &Path,
    relative: &str,
    bytes: &[u8],
    previous: Option<&[u8]>,
) -> crate::Result<()> {
    ensure!(
        read_file(root, relative)?.as_deref() == previous,
        "Target artifact changed before write"
    );
    let path = root.join(relative);
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent)?;
    directory_exists(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    ensure!(
        read_file(root, relative)?.as_deref() == previous,
        "Target artifact changed during write"
    );
    if previous.is_some() {
        file.persist(path)?;
    } else {
        file.persist_noclobber(path)?;
    }
    Ok(())
}

/// Refuses nonempty redaction instructions until an actual redaction executor exists.
pub fn redact_requires_processing(record: &CanonicalRecord) -> bool {
    record
        .consent
        .as_ref()
        .and_then(|consent| consent.redact.as_ref())
        .is_some_and(|paths| !paths.is_empty())
}
