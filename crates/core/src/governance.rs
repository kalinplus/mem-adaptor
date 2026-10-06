use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatePolicy {
    pub secrets: GateAction,
    pub high_risk_pii: GateAction,
    pub rule_allowlist: Vec<String>,
    pub origin: PolicyOrigin,
    pub user_selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateAction {
    Pass,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOrigin {
    UserChoice,
    Default,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingTier {
    Secret,
    HighRiskPii,
    PersonalFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingDisposition {
    Passed,
    Blocked,
    Allowlisted,
    ExplicitlyAllowed,
    Reported,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: String,
    pub gate_policy: GatePolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<HomeConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeConfig {
    pub format: HomeFormat,
    pub okf_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeFormat {
    Okf,
}
