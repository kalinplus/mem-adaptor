use std::fs;
use std::io::Write;
use std::path::{Component, Path};

use crate::canonical::CanonicalRecord;
use crate::engine::content_hash;
use crate::reports::*;
use anyhow::ensure;

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

pub fn mapping(canonical: &str, target: &str, rule: &str) -> TargetMapping {
    TargetMapping {
        canonical_path: canonical.into(),
        target_path: target.into(),
        rule: rule.into(),
    }
}

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
    match fs::symlink_metadata(root) {
        Ok(metadata) => ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Target must be a regular directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let mut current = root.to_owned();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "Target artifact must not be a symlink"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(fs::read(current)?))
}

pub fn artifact(root: &Path, path: &str) -> crate::Result<TargetArtifact> {
    let bytes = read_file(root, path)?;
    Ok(TargetArtifact {
        path: path.into(),
        content_hash: bytes.as_ref().map(|bytes| content_hash(bytes)),
        bytes: bytes.as_ref().map(|bytes| bytes.len() as u64),
    })
}

pub fn output_artifact(path: &str, bytes: &[u8]) -> TargetArtifact {
    TargetArtifact {
        path: path.into(),
        content_hash: Some(content_hash(bytes)),
        bytes: Some(bytes.len() as u64),
    }
}

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

pub fn redact_requires_processing(record: &CanonicalRecord) -> bool {
    record
        .consent
        .as_ref()
        .and_then(|consent| consent.redact.as_ref())
        .is_some_and(|paths| !paths.is_empty())
}
