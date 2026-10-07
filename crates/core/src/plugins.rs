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

pub struct SourceFs {
    pub root: PathBuf,
    pub files: FileInventory,
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
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim>;
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput>;
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
    fn artifacts(&self, target_ids: &[String]) -> Result<Vec<crate::reports::TargetArtifact>>;
    fn shared_artifact_paths(&self) -> &'static [&'static str];
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
