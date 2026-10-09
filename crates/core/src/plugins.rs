//! Adapter interfaces and registry between local source parsing and approved target writes.
//! Planning may inspect the target and must propagate filesystem errors, never downgrade them to absence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde_json::Value;

use crate::Result;
use crate::canonical::CanonicalRecord;
use crate::engine::WriteToken;
use crate::reports::{
    Anomaly, Capabilities, Disposition, FieldMapping, PriorWrite, ReceiptEntry, UnmappedField,
};

pub type FileInventory = BTreeMap<String, Vec<u8>>;

/// Loaded source directory and its file inventory, plus the satellite identity selected for this run.
/// The engine stamps satellite_id for home-mode runs (DEC-20); None means direct migration.
/// Readers use it to anchor record identity and scope qualifiers; they never derive or register it.
pub struct SourceFs {
    pub root: PathBuf,
    pub files: FileInventory,
    pub satellite_id: Option<String>,
}

impl SourceFs {
    pub fn file(&self, path: &str) -> &[u8] {
        &self.files[path]
    }
}

pub struct Claim {
    pub path: String,
    pub layer: String,
    pub registered_only: bool,
}

pub struct SourceRecord {
    pub canonical_id: String,
    pub source_record_id: String,
    pub source_locator: String,
    pub fields: Value,
    pub field_map: Vec<FieldMapping>,
    pub unmapped: Vec<UnmappedField>,
}

pub struct ReaderOutput {
    pub source_records: Vec<SourceRecord>,
    pub records: Vec<CanonicalRecord>,
    pub anomalies: Vec<Anomaly>,
    pub registered_count: u64,
    pub deleted_count: u64,
    pub source_unavailable: Vec<crate::reports::SourceUnavailable>,
}

pub trait Reader {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
    /// Declares whether this Reader interprets a downloaded export bundle or a live directory (DEC-20 item 5).
    /// The declaration, not the on-disk shape, decides: a Gemini Takeout is a directory-shaped export bundle.
    /// Satellite resolution uses it to require an explicit satellite for bundles and to skip path registration.
    fn source_kind(&self) -> SourceKind;
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim>;
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput>;
}

/// Classifies a source as a one-shot downloaded export bundle or a directory the user keeps in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    ExportBundle,
    Directory,
}

pub struct Planned {
    pub record: CanonicalRecord,
    pub disposition: Disposition,
    pub target_id: String,
    pub previous_write: Option<PriorWrite>,
    pub duplicate_write: Option<crate::reports::DuplicateWrite>,
    pub target_map: Vec<crate::reports::TargetMapping>,
}

pub struct Written {
    pub canonical_id: String,
    pub target_id: String,
    pub target_hash: String,
}

#[derive(Default)]
pub struct WriteResult {
    pub written: Vec<Written>,
    pub artifacts: Vec<crate::reports::TargetArtifact>,
}

pub struct ReadBack {
    pub canonical_id: String,
    pub record: CanonicalRecord,
}

/// Holds one phase-local observation; an untracked native payload may have a hash without a canonical record.
pub struct TargetState {
    pub record: Option<CanonicalRecord>,
    pub target_hash: Option<String>,
    /// Non-fatal damage classification for planning (DEC-21 A/D): `None` means healthy or absent, and
    /// Writers that cannot classify leave it unset so the engine keeps its legacy behaviour.
    pub classification: Option<TargetClassification>,
    /// Home-file-side pointers of fields edited relative to the envelope evidence (DEC-21 A), filled by
    /// Writers that attribute home edits; empty for healthy-unattributed states and non-OKF writers.
    pub home_changed_fields: Vec<String>,
    /// Record hash the target file's own tool-owned block carries, before any home-side adoption
    /// (DEC-21 A). The engine compares it with the recorded basis to attribute an envelope edit.
    pub envelope_hash: Option<String>,
}

/// Reports why a managed target file is not a healthy managed record (DEC-21 A/D).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetClassification {
    /// The management envelope is gone: the file is an ordinary user note at our target path.
    Unmanaged,
    /// The envelope parses but fails schema or consistency validation.
    ManagedInvalid,
}

pub trait Writer {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn location(&self) -> &Path;
    fn capabilities(&self) -> Capabilities;
    /// Projects a record and target disposition without writes; inspection failures abort planning.
    fn plan(&self, record: &CanonicalRecord, previous: Option<&ReceiptEntry>) -> Result<Planned>;
    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<WriteResult>;
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>>;
    fn inspect(&self, target_id: &str) -> Result<Option<CanonicalRecord>>;
    fn target_hash(&self, target_id: &str) -> Result<Option<String>>;
    /// Inspects a collection within one phase; overrides may parse once but must not cache across execution boundaries.
    fn inspect_many(&self, target_ids: &[String]) -> Result<BTreeMap<String, TargetState>> {
        target_ids
            .iter()
            .map(|id| {
                Ok((
                    id.clone(),
                    TargetState {
                        record: self.inspect(id)?,
                        target_hash: self.target_hash(id)?,
                        classification: None,
                        home_changed_fields: Vec::new(),
                        envelope_hash: None,
                    },
                ))
            })
            .collect()
    }
    /// Rechecks native hashes after read-back without relying on an earlier inspection snapshot.
    fn target_hashes(&self, target_ids: &[String]) -> Result<BTreeMap<String, Option<String>>> {
        target_ids
            .iter()
            .map(|id| Ok((id.clone(), self.target_hash(id)?)))
            .collect()
    }
    fn artifacts(&self, target_ids: &[String]) -> Result<Vec<crate::reports::TargetArtifact>>;
    fn shared_artifact_paths(&self) -> &'static [&'static str];
    /// Renders the exact bytes a write of `planned` would produce, without writing (DEC-21 B rule five:
    /// convergence needs a byte-identical projection including sticky fields). `None` means this
    /// Writer cannot project, so the engine keeps its legacy unresolved treatment instead of guessing.
    fn project(&self, planned: &Planned) -> Result<Option<Vec<u8>>> {
        let _ = planned;
        Ok(None)
    }
}

#[derive(Default)]
pub struct Registry {
    pub readers: Vec<Box<dyn Reader>>,
    pub writers: BTreeMap<String, Box<dyn Writer>>,
}

impl Registry {
    pub fn register_reader(&mut self, reader: impl Reader + 'static) -> Result<()> {
        ensure!(
            !self
                .readers
                .iter()
                .any(|existing| existing.id() == reader.id()),
            "Reader already registered"
        );
        self.readers.push(Box::new(reader));
        Ok(())
    }

    pub fn register_writer(&mut self, target: String, writer: impl Writer + 'static) -> Result<()> {
        ensure!(
            !self.writers.contains_key(&target),
            "Target already registered"
        );
        self.writers.insert(target, Box::new(writer));
        Ok(())
    }

    pub fn writer(&self, target: &str) -> Result<&dyn Writer> {
        self.writers
            .get(target)
            .map(|writer| writer.as_ref())
            .context("Target Writer is not registered")
    }
}
