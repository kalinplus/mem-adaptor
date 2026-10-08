//! Defines policy, finding, approval, and configuration shapes shared by planning, execution, and reports.
//! The CLI currently records local approval; these types do not provide authentication, signatures, or a ledger.
//! Configuration types are the shape contract; TOML persistence and the satellite registry lifecycle live in
//! `crate::satellite`, and the `init` command belongs to the CLI.

use serde::{Deserialize, Serialize};

/// Records selected/default outbound treatment; available policy fields do not imply complete PII detection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatePolicy {
    pub secrets: GateAction,
    pub high_risk_pii: GateAction,
    pub rule_allowlist: Vec<String>,
    pub origin: PolicyOrigin,
    pub user_selected: bool,
}

/// Chooses whether detected findings permit or prevent the affected record from being exported.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateAction {
    Pass,
    Block,
}

/// Distinguishes user selection from defaults rather than inferring consent from silent execution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOrigin {
    UserChoice,
    Default,
}

/// Locates a detection and its policy outcome without copying the detected value into reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub rule_id: String,
    pub tier: FindingTier,
    pub field_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_hash: Option<String>,
    pub byte_span: ByteSpan,
    pub disposition: FindingDisposition,
}

/// Locates detection bytes in the relevant value or key, not character offsets in a rendered report.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}

/// Names finding categories; the vocabulary alone does not implement detectors for every category.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingTier {
    Secret,
    HighRiskPii,
    PersonalFact,
}

/// Explains a finding's treatment independently of a record's disposition or target verification.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingDisposition {
    Passed,
    Blocked,
    Allowlisted,
    ExplicitlyAllowed,
    Reported,
}

/// Records who approved a specific plan digest and when, not whether any target write succeeded.
/// The approval principal is not a copied source owner; the current local receipt is not a signed credential.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalReceipt {
    pub schema_version: String,
    pub receipt_id: String,
    pub plan_digest: String,
    pub approved_at: String,
    pub backend: String,
    pub approver: String,
}

/// Describes validated policy/home settings without implementing loading or saving a user's config file.
/// Loading, saving, and the satellite registry lifecycle live in `crate::satellite`; this type is the shape contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: String,
    pub gate_policy: GatePolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<HomeConfig>,
    /// Home-mode satellite registry (DEC-20 item 4); absent in direct-mode user configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub satellites: Option<Vec<SatelliteEntry>>,
}

/// Binds one issued satellite ID to its display label, detected source system, and current path binding.
/// The ID is issued once and never changes; `path` is a rebindable resolution hint and is absent for
/// export-bundle satellites whose download location is not stable. The label is display-only and mutable:
/// it never participates in any hash and is not required to be unique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SatelliteEntry {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub system: String,
    pub created_at: String,
}

/// Describes the chosen home format/version without establishing synchronization or history storage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeConfig {
    pub format: HomeFormat,
    pub okf_version: String,
}

/// Names the currently allowed home-format contract.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeFormat {
    Okf,
}
