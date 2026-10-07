//! Defines the plan and receipt contracts exchanged between the engine, CLI, and future conformance tools.
//! Plans predict per-record treatment; receipts associate actual writes and read-back evidence with that plan.
//! These types describe evidence, not approval enforcement, authenticated signatures, or rollback.

use serde::{Deserialize, Serialize};

use crate::canonical::{EvidenceLevel, ReembedPlan, Verdict};
use crate::governance::{Finding, GatePolicy};

/// Describes how a record is treated for one target, independently of whether a write was verified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Accepted,
    Transformed { changes: Vec<Change> },
    Omitted { reason: OmissionReason },
    Unresolved { reason: UnresolvedReason },
    Rejected { rule: String },
}

/// Identifies a field-level transformation without embedding the original sensitive value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub field_path: String,
    pub kind: ChangeKind,
}

/// Classifies a declared mapping or loss; this vocabulary does not perform the transformation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Reformatted,
    FieldOmitted,
    Mapped,
    Referenced,
    MetadataChanged,
}

/// Explains why no new write is scheduled, including references to earlier or representative writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case", deny_unknown_fields)]
pub enum OmissionReason {
    DuplicateOf { canonical_id: String },
    AlreadyMigrated,
    DeletedInTarget,
    TargetUnsupported { field: String },
    VerdictExcluded { cluster_id: String },
    SourceMissing,
    SecretReferenceUnsupported,
}

/// Explains a decision still needed before writing; the engine must not silently resolve it.
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

/// Describes read-back evidence separately from disposition; verified is limited to compared supported fields.
/// Unverifiable is a contract alternative, not a currently implemented fallback for read-back errors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verification {
    Verified,
    Mismatch { diff: Vec<FieldDiff> },
    Unverifiable { why: String },
}

/// Locates a read-back difference using paths and optional hashes rather than exposing field values.
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

/// Names the kind of discrepancy represented by a field diff.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffKind {
    Missing,
    Changed,
    Unexpected,
}

/// Records a Writer's declared coverage and read-back/update support, not independently proven capabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub supported_fields: Vec<String>,
    pub unsupported_fields: Vec<String>,
    pub read_back: bool,
    pub update: bool,
}

/// Associates a logical target with its Writer, location, and approval-time or receipt-time artifact evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSpec {
    pub id: String,
    pub location: String,
    pub writer: String,
    pub artifacts: Vec<TargetArtifact>,
}

/// Describes a relative native artifact; absent hash and size represent a missing file, not an empty file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetArtifact {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// Explains canonical-to-native field correspondence and its mapping rule without embedding native payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMapping {
    pub canonical_path: String,
    pub target_path: String,
    pub rule: String,
}

/// Identifies the Writer version and declared capabilities included in the execution basis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriterSpec {
    pub id: String,
    pub version: String,
    pub capabilities: Capabilities,
}

/// Identifies a Reader version involved in interpreting the source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterVersion {
    pub id: String,
    pub version: String,
}

/// Locates the source and names its export/Reader context; it is not a copy of all source contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub location: String,
    pub system: String,
    pub export_version: String,
    pub adapters: Vec<AdapterVersion>,
}

/// Explains which source files were claimed, only registered, or not recognized during planning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInventory {
    pub files: Vec<InventoryFile>,
    pub state: InventoryState,
}

/// Distinguishes an empty input inventory from one containing data, not complete conversion coverage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryState {
    DataPresent,
    Empty,
}

/// Associates file-byte evidence with Reader coverage and optional registration/deletion counts.
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

/// Distinguishes parsing claims, registration-only handling, and files no Reader recognized.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    Claimed,
    Unclaimed,
    RegisteredOnly,
}

/// Records declared model-call facts and affected records/fields; its presence does not implement model calls.
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

/// Distinguishes explicit model opt-in from calls attributed to a target.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallOrigin {
    OptIn,
    TargetTriggered,
}

/// Explains source-to-canonical field coverage separately from canonical-to-target mappings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMapping {
    pub source_path: String,
    pub canonical_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

/// Makes a source coverage gap visible without embedding the original field value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnmappedField {
    pub source_path: String,
    pub reason: UnmappedReason,
}

/// Classifies why a source field lacks declared coverage; unknown does not necessarily mean discarded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmappedReason {
    SourceUnknown,
    EngineUnreported,
    TargetUnsupported,
    RegisteredOnly,
}

/// Predicts treatment for one canonical record and target, with coverage, losses, findings, and prior evidence.
/// A preview is display-only; neither accepted nor prior_write proves a new write happened in this run.
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

/// Binds per-target treatment, mapping, and relevant historical evidence into the plan digest.
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

/// Associates stable identity, body/full-record hashes, and target predictions for approval comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestRecord {
    pub canonical_id: String,
    pub content_hash: String,
    pub record_hash: String,
    pub predictions: Vec<Prediction>,
}

/// Defines the hashed execution basis, excluding current presentation-only run IDs and timestamps.
/// A previous receipt's complete bytes remain bound because its carried history is an execution dependency.
/// Source-manifest and report-entry consistency require additional engine checks, not just this hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestInputs {
    pub records: Vec<DigestRecord>,
    pub targets: Vec<TargetSpec>,
    pub writers: Vec<WriterSpec>,
    pub gate_policy: GatePolicy,
    /// Binds every fact carried from the exact loaded receipt, including records absent from this source round.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_receipt_hash: Option<String>,
}

/// Records source-file hashes for change checks without copying the source bundle into the report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub source_system: String,
    pub export_version: String,
    pub files: Vec<ManifestFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
}

/// Associates a source-relative file path with its exact-byte hash and size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub content_hash: String,
    pub bytes: u64,
}

/// Locates a reported parsing anomaly without treating it as a successful record conversion.
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

/// Makes unavailable source layers and the strength of their supporting evidence explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUnavailable {
    pub system: String,
    pub layer: String,
    pub reason: String,
    pub evidence_level: EvidenceLevel,
}

/// Explains proposed work before approval: input/target context, coverage, policy, losses, and execution basis.
/// A schema-valid plan is neither permission to write nor proof that migration succeeded.
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

/// Associates actual treatment with record/target identity and optional current verification.
/// Skipped entries omit current verification but may retain historical or representative write evidence.
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

/// References a duplicate representative's write without claiming that the alias itself was written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DuplicateWrite {
    pub canonical_id: String,
    pub prior_write: PriorWrite,
}

/// Carries a record's earlier actual write and verification for later comparison and deletion protection.
/// Historical verified evidence is not a fresh verification of a skipped record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorWrite {
    pub target_id: String,
    pub content_hash: String,
    pub record_hash: String,
    pub target_hash: String,
    pub verification: Verification,
}

/// Reports actual dispositions and available write/read-back evidence, linked to the approved plan.
/// Returning this value does not persist it; a later CLI save failure cannot undo target writes.
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
