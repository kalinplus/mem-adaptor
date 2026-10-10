# M1：schema v0 字段方案（已确认、已实现）

这是 [implementation-plan.md](implementation-plan.md) §6 要过目的 M1 方案，
已于 2026-10-05 由用户确认，正本在 `schema/`，对应 Rust 类型与一致性测试已通过验收。
以 [design.md](design.md) §2、§3 为准；具体子字段以手写正本为准，本方案不是另一份格式规范。
canonical schema 只做内部校验，不能变成对外记忆交换格式。
M3、M4 的追加契约已确认，见 [m3-contract-proposal.md](m3-contract-proposal.md)、
[m4-reader-proposal.md](m4-reader-proposal.md)；v0 尚未发布，
直接更新正本，不兼容旧计划/回执。

## 实现路径

1. **一次写全设计中的字段，D1 未用字段可选（已选择）**：符合既定 M1 计划，尽早固定报告契约；
   初版字段较多，但向量层与冲突层不会等到 D2 再补。
2. **先写 D1 最小字段，后续升级（未选择）**：初版更小，但报告契约会较快变更，不符合当前 M1 的既定范围。

## 通用约定

- 5 个正本：`canonical-record`、`plan-report`、`receipt-report`、`approval-receipt`、`config`；
  文件名加 `.schema.json`，使用 JSON Schema draft 2020-12。
- 字段用 `snake_case`；报告、审批凭证、配置有 `schema_version: "0.1.0"`；
  报告另有 `canonical_model_version`，不把内部版本与报告版本混在一起。
- 缺失的可选值省略，不用 `null`；对象按声明字段封闭，唯独 `source_extra` 允许原样未知 metadata，
  嵌套 null 等源值不改写；同时进入未承载清单并列保留位置。
- 哈希用带算法前缀的字符串：`sha256:<64 位小写十六进制>`。
- 时间用 RFC 3339；源侧只有日期或 unknown 时，不补造时刻，保留到源字段说明。
- 字段路径用 JSON Pointer（JSON 指针）；密钥命中的 `byte_span` 是 UTF-8 字节的左闭右开区间。
- `canonical_id` 继续按实现计划的「源系统 + NUL + 源 id」派生；
  **取 SHA-256 前 20 字节，转无 padding 的小写 base32，共 32 字符（已确认）**。

## 内部 canonical record

**必需**：`canonical_id`、`source`、`source_record_id`、`source_locator`、`scope`、
`content`、`content_hash`、`dna_class`、`provenance`、`evidence_level`。

`source` 必含 `system`、`adapter_version`、`export_version`，格式版本未知时明确记 `"unknown"`。
`scope` 用设计中的 `user/project/agent/session/tenant/wing/room`；`owner_declared` 仅表示不可信的源声明。
`dna_class` 用 `dna/standard`；`evidence_level` 用 `measured/official/third_party/inferred`，
用户可读解释对应「实测/官方文档/第三方核对/推断」。

**可选字段按设计保留**：

| 组 | 字段 |
|---|---|
| 地址 | `scope_qualifier`、`owner_declared` |
| 内容 | `source_kind`（源有则原样保留）、`tags`、`entities`、`relations` |
| 源 metadata | `source_extra`，非空 JSON 对象；无值时省略，不是记录归档 |
| 时间 | `created_at`、`updated_at`、`observed_at`、`valid_from`、`valid_to`、`expires_at`、`ttl` |
| 治理 | `consent`、`approval`、`sensitive_findings`、`deletion_intent`、`tombstone` |
| 向量 | `embedding`、`reembed_plan` |
| 冲突 | `conflict_cluster_id`、`conflict_candidates`、`verdict` |

`embedding` 出现时必含 `model` 和 `dim`；`vector`、`normalized` 可选，
这样 OKF 回读能表示「保留了模型与维度，但没有向量」。向量丢弃必须进处置改写清单，不能算完整 accepted。
`verdict` 只表达 `keep` 或 `needs_more_context`，不设自动 merge。

## 计划报告

| 必需顶层字段 | 内容 |
|---|---|
| `schema_version`、`canonical_model_version`、`run_id`、`created_at` | 报告版本和运行信息 |
| `source`、`source_inventory` | 源定位、格式版本、adapter 版本；认领/无人认领/只登记的文件与会话计数 |
| `targets`、`writers` | 每个目标的定位、Writer 版本、承载字段与回读能力 |
| `model_calls` | 显式模型调用事实，D1 为 `[]` |
| `gate_policy` | 密钥、高危 PII 的 pass/block，规则白名单，来源及是否经过用户选择 |
| `entries` | 按记录和目标的逐条预测、字段映射、未承载字段、命中、证据等级 |
| `source_unavailable`、`anomalies`、`warnings` | 源不可得、坏条目、风险提示，即使为空也有字段 |
| `bundle_manifest` | 源系统、导出时间（可选）、格式版本和文件摘要 |
| `digest_inputs`、`plan_digest` | 完整摘要输入和 JCS（JSON Canonicalization Scheme，JSON 规范化方案）+ SHA-256 摘要 |

`entries` 不内联 canonical 正文或源记录。可选 `content_preview` 必须先遮蔽所有命中值，
再截取展示；原文留在源侧，`apply` 重读源数据后重算计划。报告不能成为保存原文的内部归档格式。
摘要保持实现计划的 `{records, targets, writers, gate_policy}`，记录按 `canonical_id` 排序，
时间戳和 `run_id` 不参与。
摘要记录必带完整 canonical 的 `record_hash`；逐目标预测与计划 entry 可带 `prior_write`，
计划可带 `previous_receipt_ref` 供 apply 重读同一历史输入。
M4 的字段映射可带 `rule`，说明分类或 metadata 保留规则；源清单可带非负 `deleted_count`。
Reader 内部的原样字段、映射与未承载清单按 `canonical_id` 关联，不再文件级共用。

## 处置与回执

处置用带 `status` 标签的五种互斥对象，Rust 对应穷举 enum：

- `accepted`：无额外原因。
- `transformed`：必带非空 `changes`，列字段路径和改写类型，不复制敏感原值。
- `omitted`：必带 `reason`，例如 `duplicate_of/already_migrated/deleted_in_target/target_unsupported`；
  重复原因必带被重复条目的 id，不支持字段原因必带字段路径。
- `unresolved`：必带 `reason`，例如 `conflict/dna_unsupported`。
- `rejected`：必带触发的 `rule`。

回执包含计划的版本、来源、目标、策略、模型调用、manifest 和摘要信息，加
`plan_ref`、`approval_receipt_ref`、`entries`、`verdicts`。
逐条 `entries` 记 `source_record_id ↔ canonical_id ↔ target_id`、`content_hash`、
实际处置、命中、证据等级；真实写入的条目要求 `target_id`、本次验证结果与 `prior_write`。
验证结果为 `verified`、带非空 `diff` 的 `mismatch`、带 `why` 的 `unverifiable`。
未写入不假称 verified，也不伪造目标 id。
`prior_write` 仅记录此前真实发生的目标 id、正文哈希、完整 metadata 哈希与验证结果；
跳过时可沿用，真实写入时由新回读依据替代，供后续幂等与防复活使用。

## 审批凭证与配置

- 审批凭证：`schema_version`、`receipt_id`、`plan_digest`、`approved_at`、`backend`、
  `approver`；本地审批主体由交互确认获得，不从 `owner_declared` 推导。
- 配置：`schema_version`、`gate_policy`；家模式额外记 `home` 的目标格式和固定 OKF 版本。
  TOML 解析成该结构后校验，不自建独立状态账本。

## M1 验收

- 每份 schema 至少有有效向量和无效向量，无效向量在配套期望文件里注明违反的约束。
- 有效向量可反序列化成 Rust 类型，再序列化仍通过 schema。
- Rust 构造的完整与最小示例通过 schema。
- 无效向量被 schema 拒绝，包括缺必需字段、未知处置、缺原因/改写、embedding 缺模型或维度。
- 验证器检查时间格式；字节 span 次序、向量长度与 dim 等跨字段约束由引擎验证，不假称 JSON Schema 能做。
