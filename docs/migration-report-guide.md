# M1-C：迁移报告怎样衔接

这轮只看用途、流程和可信边界，不逐字段审计。内部记录的背景见
[canonical-record-guide.md](canonical-record-guide.md)。

## 1. 两份报告，加一份审批凭证

| 产物 | 回答的问题 | 不能据此断言 |
|---|---|---|
| 计划报告 `PlanReport` | 读到了什么，准备向哪里写，哪些记录转换、跳过、待决或拒绝，有什么损失与风险？ | 已批准、已写入或已迁移成功 |
| 审批凭证 `ApprovalReceipt` | 谁在什么时候批准了哪个执行依据的摘要？ | 写入已经发生，或当前本地凭证有签名/认证后端 |
| 回执报告 `ReceiptReport` | 实际处置是什么，写到了哪里，回读有什么证据，下一轮可以沿用什么历史？ | 所有源内容都完整迁移，或目标平台后续使用效果已经验证 |

当前命令行入口（Command-Line Interface，CLI）的顺序：

```text
读取/归一 → 计划报告落盘 → 人确认该依据，审批凭证落盘
→ 引擎重算并核对依据 → 合格记录写入、回读 → 回执报告落盘
```

**批准与执行成功是两件事，写入与回执保存也是两件事。**
审批绑定 `plan_digest`；源记录（含元数据）、目标状态、Writer 版本/能力或策略改变，
旧批准不能继续使用，需要重新规划并批准。引擎还会核对源文件清单和计划条目，
不是只比较报告路径或顶层摘要字符串。新计划使用新输出文件，不覆盖旧凭据。

报告信息按用途理解即可：身份与关联用于找到同一条记录和目标；
清单、映射、损失和异常用于说明覆盖范围；策略、命中和模型调用事实用于解释风险；
摘要、产物哈希、当前回读与历史写入依据用于核对执行，不必背字段。

## 2. 正常例子：计划不是收据

在全新的隔离合成目录中，只有 `source/short.md`，正文为：

```text
Prefer short answers.
```

目标选择开放知识格式（Open Knowledge Format，OKF）目录 `home`：

```sh
mem-adaptor plan source --to okf:home --report plan.json
mem-adaptor apply plan.json --yes
```

| 时间点 | 可观察产物与事实 |
|---|---|
| `plan` 完成 | `plan.json` 预计这条为 `accepted`，列出源到内部记录、内部记录到目标的映射；目标目录尚未创建 |
| 明确批准 | `plan.approval.json` 绑定计划摘要；默认放行策略仍注明来自默认值，不把批准混成策略选择 |
| `apply` 完成 | 目标记忆文件保留正文；`plan.receipt.json` 关联同一条记录、目标位置与审批引用，当前回读为 `verified`，并保存实际写入依据 |

`accepted` 是“按映射承载这条”，不是“已经验证”。
如果目标不支持某个可丢弃字段，例如向量，计划可以是 `transformed` 并明确列出损失，
回执仍可以是 `verified`：验证的是**声明支持的字段与预计映射结果**，不是全字段无损。
当前跳过的条目没有本次 `verification`；保留的历史 `prior_write.verification`
只说明早先的写入，不把历史证据冒充本轮重新验证。

## 3. 拒绝例子：计划后源变了

另取一个全新的隔离目录。先用同一正文生成 `plan.json`，再将源正文改为
`Prefer detailed answers.`，然后执行旧计划的 `apply --yes`。

引擎重算得到不同的执行依据，返回摘要不匹配错误。本例中目标仍不存在，
也没有 `plan.receipt.json`，但 **`plan.approval.json` 已存在**：
CLI 在重算前就保存了“用户批准旧依据”这一事实。拒绝写目标不等于完全没有写盘。
应使用新文件重新规划并批准，不删除旧凭据来盲目重试。

这属于一次执行的写前拒绝，不是把所有条目改成 `rejected`：
逐条 `rejected` 指某条违反闸门/约束；其他合格条目仍可能正常迁移。

**不能将本例推广成“失败或无回执，目标一定没变”。**
写后产物核对、回读或最终保存回执都可能失败，目标可能已变；当前没有整次迁移事务、
回滚或安全恢复承诺。回读差异会记为 `mismatch`，不能当作 `verified`；
`unverifiable` 表示不能得到回读证据，是 schema 的合法状态，但当前两个文件 Writer
没有把回读错误自动转换成这种回执的实现，错误会传播。

## 4. 有限阅读入口与证据边界

用户先读上面三个小节即可；想对应源码时按这个顺序：

1. [reports.rs](../crates/core/src/reports.rs)：`PlanReport` → `ReceiptReport` → `Disposition` / `Verification` 的意图说明。
2. [governance.rs](../crates/core/src/governance.rs)：只看 `ApprovalReceipt`，不要把它当成第三份成功报告。
3. [main.rs](../crates/cli/src/main.rs)：`run` 中审批、引擎调用和回执保存的边界；核心入口是
   [engine.rs](../crates/core/src/engine.rs) 的 `plan_with_previous` / `apply`。
4. [migration.rs](../crates/cli/tests/migration.rs)：优先看
   `approved_apply_produces_verified_schema_valid_receipt` 与 `changed_source_is_refused_before_target_write`。
   后者原有断言没有检查审批文件；上述审批存在性需要另核对实际 CLI 产物，不能冒称原测试已断言。

该测试文件依次分为报告生命周期/摘要、写前拒绝、覆盖/隐私、历史依据、ZIP 集成；
这是阅读分类，不是 Rust 的测试执行顺序。所有测试和 helper 有就地英文说明。
结构证据见 [schema_consistency.rs](../crates/core/tests/schema_consistency.rs)：有效样例往返、
必需字段删除、无效 mutations 和状态枚举。五份 schema 与
[vectors 说明](../schema/vectors/README.md) 是机器契约；示例中的占位哈希不是实际迁移凭据。

这些证据不代表完整独立符合性验证（conformance）、真实导出验收、目标检索质量或全面个人信息
（Personally Identifiable Information，PII）检测。报告有正文预览等敏感信息；
检测到的密钥模式被遮盖，不等于完整隐私清洗，默认放行也不等于目标正文被脱敏。
配置类型不等于配置持久化，审批类型不等于签名治理后端。
所有写后故障、回执保存失败的统一提示与安全重试也尚不能由现有测试完整证明。
