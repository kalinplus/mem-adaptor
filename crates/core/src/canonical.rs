//! Internal record vocabulary passed from Readers through the local engine to Writers.
//! Required fields identify, locate, and describe each record; optional fields retain source-specific semantics.
//! Serialization supports internal hashing, validation, and adapter extensions, not a public interchange standard.
//! These types do not validate values, grant authority, or implement the capabilities their fields describe.

use serde::{Deserialize, Serialize};

use crate::governance::Finding;

/// Common adapter-independent record; the hand-authored schema defines its serialized constraints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalRecord {
    // Required: identity and source address, body integrity, preservation class, and traceable provenance.
    pub canonical_id: String,
    pub source: SourceIdentity,
    pub source_record_id: String,
    pub source_locator: String,
    pub scope: Scope,
    pub content: String,
    pub content_hash: String,
    pub dna_class: DnaClass,
    pub provenance: Provenance,
    pub evidence_level: EvidenceLevel,
    // Optional: source declarations and descriptive metadata, not an authenticated owner or invented source facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_qualifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_declared: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_extra: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<Entity>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<Relation>>,
    // Optional: record, observation, validity, and retention times remain distinct.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,
    // Optional: governance declarations and findings are inputs to policy, not execution approval or deletion actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consent: Option<Consent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitive_findings: Option<Vec<Finding>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_intent: Option<DeletionIntent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tombstone: Option<Tombstone>,
    // Optional: vector provenance and a proposed rebuild do not imply a model call or vector-search capability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Embedding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reembed_plan: Option<ReembedPlan>,
    // Optional: conflict evidence and human decisions do not authorize automatic conflict resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_cluster_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_candidates: Option<Vec<ConflictCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

/// Identifies the source system and adapter/export versions without interpreting its native record IDs.
/// The optional satellite anchors record identity in home mode (DEC-20); it is absent in direct migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub system: String,
    pub adapter_version: String,
    pub export_version: String,
    /// Registered satellite ID (8 lowercase base32 chars) participating in canonical_id.
    /// The stored value is authoritative on home read-back; it is never re-derived from run context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satellite_id: Option<String>,
}

/// Source-side address category, never an access-control boundary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    User,
    Project,
    Agent,
    Session,
    Tenant,
    Wing,
    Room,
}

/// Distinguishes protected identity/preference/procedure semantics from ordinary records during mapping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnaClass {
    Dna,
    Standard,
}

/// Labels the basis of an adapter's interpretation; it is not a truth or confidence score for the memory.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    Measured,
    Official,
    ThirdParty,
    Inferred,
}

/// Retains a typed source entity without claiming entity extraction or cross-source identity resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Carries a source-declared typed edge rather than implementing a graph store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub kind: String,
    pub source: String,
    pub target: String,
}

/// Records who or what produced the record and how, keeping source references separate from content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub actor: String,
    pub actor_kind: ActorKind,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<Evidence>>,
}

/// Describes a provenance actor's role without authenticating that actor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
    Model,
    Import,
    Scan,
}

/// Points to supporting source material; an optional source weight is not computed by this model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// Carries source export, retention, redaction, and memory-enable declarations for explicit policy handling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exportable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redact: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_enabled: Option<bool>,
}

/// Retains a record's declared approval state; it cannot replace the engine's approval of a migration plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalState {
    pub state: ApprovalStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_ref: Option<String>,
}

/// Vocabulary for the retained record-level approval declaration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

/// Expresses requested lifecycle handling, not an instruction to execute a target deletion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionIntent {
    Delete,
    Redact,
    Deprecate,
}

/// Preserves source deletion evidence without performing revocation or downstream propagation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tombstone {
    pub deleted_at: String,
    pub source_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

/// Keeps model and dimension with optional vector values; absence of a vector must not trigger synthesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Embedding {
    pub model: String,
    pub dim: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized: Option<bool>,
}

/// Describes a separate, explicit re-embedding proposal and its stated quality impact, not completed work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReembedPlan {
    pub canonical_ids: Vec<String>,
    pub model: String,
    pub dim: u64,
    pub quality_impact: String,
}

/// Retains a candidate and comparison basis without choosing which memory is correct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictCandidate {
    pub canonical_id: String,
    pub basis: String,
}

/// Carries an explicit decision or request for more context so later runs need not invent a verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verdict {
    Keep {
        cluster_id: String,
        canonical_ids: Vec<String>,
    },
    NeedsMoreContext {
        cluster_id: String,
    },
}
