use serde::{Deserialize, Serialize};

use crate::canonical::{EvidenceLevel, ReembedPlan, Verdict};
use crate::governance::{Finding, GatePolicy};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Accepted,
    Transformed { changes: Vec<Change> },
    Omitted { reason: OmissionReason },
    Unresolved { reason: UnresolvedReason },
    Rejected { rule: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub field_path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Reformatted,
    FieldOmitted,
    Mapped,
    Referenced,
    MetadataChanged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum OmissionReason {
    DuplicateOf { canonical_id: String },
    AlreadyMigrated,
    DeletedInTarget,
    TargetUnsupported { field: String },
    VerdictExcluded { cluster_id: String },
    SourceDeleted,
    SecretReferenceUnsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum UnresolvedReason {
    Conflict { cluster_id: String },
    DnaUnsupported { field: String },
    MemoryDisabled,
    DeletionNeedsDecision,
    TargetModified,
    TargetUntracked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verification {
    Verified,
    Mismatch { diff: Vec<FieldDiff> },
    Unverifiable { why: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDiff {
    pub field_path: String,
    pub kind: DiffKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Missing,
    Changed,
    Unexpected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub supported_fields: Vec<String>,
    pub unsupported_fields: Vec<String>,
    pub read_back: bool,
    pub update: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    pub id: String,
    pub location: String,
    pub writer: String,
    pub artifacts: Vec<TargetArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetArtifact {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMapping {
    pub canonical_path: String,
    pub target_path: String,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterSpec {
    pub id: String,
    pub version: String,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterVersion {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub location: String,
    pub system: String,
    pub export_version: String,
    pub adapters: Vec<AdapterVersion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventory {
    pub files: Vec<InventoryFile>,
    pub state: InventoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryState {
    DataPresent,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryFile {
    pub path: String,
    pub content_hash: String,
    pub bytes: u64,
    pub status: InventoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registered_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    Claimed,
    Unclaimed,
    RegisteredOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCall {
    pub origin: ModelCallOrigin,
    pub model: String,
    pub count: u64,
    pub canonical_ids: Vec<String>,
    pub field_paths: Vec<String>,
    pub remote: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallOrigin {
    OptIn,
    TargetTriggered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMapping {
    pub source_path: String,
    pub canonical_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnmappedField {
    pub source_path: String,
    pub reason: UnmappedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmappedReason {
    SourceUnknown,
    EngineUnreported,
    TargetUnsupported,
    RegisteredOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanEntry {
    pub canonical_id: String,
    pub source_record_id: String,
    pub source_locator: String,
    pub content_hash: String,
    pub target: String,
    pub disposition: Disposition,
    pub field_map: Vec<FieldMapping>,
    pub target_map: Vec<TargetMapping>,
    pub unmapped: Vec<UnmappedField>,
    pub sensitive_findings: Vec<Finding>,
    pub evidence_level: EvidenceLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reembed_plan: Option<ReembedPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_write: Option<PriorWrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_write: Option<DuplicateWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    pub target: String,
    pub disposition: Disposition,
    pub target_map: Vec<TargetMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_write: Option<PriorWrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_write: Option<DuplicateWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestRecord {
    pub canonical_id: String,
    pub content_hash: String,
    pub record_hash: String,
    pub predictions: Vec<Prediction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestInputs {
    pub records: Vec<DigestRecord>,
    pub targets: Vec<TargetSpec>,
    pub writers: Vec<WriterSpec>,
    pub gate_policy: GatePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub source_system: String,
    pub export_version: String,
    pub files: Vec<ManifestFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub content_hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anomaly {
    pub source_locator: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUnavailable {
    pub system: String,
    pub layer: String,
    pub reason: String,
    pub evidence_level: EvidenceLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReport {
    pub schema_version: String,
    pub canonical_model_version: String,
    pub run_id: String,
    pub created_at: String,
    pub source: SourceSpec,
    pub source_inventory: SourceInventory,
    pub targets: Vec<TargetSpec>,
    pub writers: Vec<WriterSpec>,
    pub model_calls: Vec<ModelCall>,
    pub gate_policy: GatePolicy,
    pub entries: Vec<PlanEntry>,
    pub source_unavailable: Vec<SourceUnavailable>,
    pub anomalies: Vec<Anomaly>,
    pub warnings: Vec<String>,
    pub bundle_manifest: BundleManifest,
    pub digest_inputs: DigestInputs,
    pub plan_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_receipt_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEntry {
    pub canonical_id: String,
    pub source_record_id: String,
    pub source_locator: String,
    pub content_hash: String,
    pub target: String,
    pub disposition: Disposition,
    pub target_map: Vec<TargetMapping>,
    pub sensitive_findings: Vec<Finding>,
    pub evidence_level: EvidenceLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<Verification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_write: Option<PriorWrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_write: Option<DuplicateWrite>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateWrite {
    pub canonical_id: String,
    pub prior_write: PriorWrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorWrite {
    pub target_id: String,
    pub content_hash: String,
    pub record_hash: String,
    pub target_hash: String,
    pub verification: Verification,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptReport {
    pub schema_version: String,
    pub canonical_model_version: String,
    pub run_id: String,
    pub created_at: String,
    pub source: SourceSpec,
    pub targets: Vec<TargetSpec>,
    pub writers: Vec<WriterSpec>,
    pub model_calls: Vec<ModelCall>,
    pub gate_policy: GatePolicy,
    pub bundle_manifest: BundleManifest,
    pub plan_digest: String,
    pub plan_ref: String,
    pub approval_receipt_ref: String,
    pub entries: Vec<ReceiptEntry>,
    pub verdicts: Vec<Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_receipt_ref: Option<String>,
}
