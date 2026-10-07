# Auto review 规则（机器规则正本）

本文是自动 review 的规则正本，供本仓库委派的 review agent、OCR（Open Code Review，阿里开源 AI 代码审查工具）
以及以后接入的 CI review 共同读取。工具入口只引用或派生本文，不另写规则。

本文只决定“机器应该找什么、怎样报告”。契约是否值得接受、风险能否承担，由人按
[review-workflow.md](review-workflow.md) §4 判断；格式、lint 与编译错误由 `cargo fmt`、Clippy 和测试负责，不在此重复。
行为要求的正本仍是 [testing-policy.md](testing-policy.md) 与 [AGENTS.md](../AGENTS.md) 铁律，本文只把它们映射到路径。

## 1. 范围与输出

### 两种模式

| 模式 | 用于 | 报告什么 |
|---|---|---|
| 改动模式 | 每个 PR 的 diff | 只报本次改动引入或暴露的问题 |
| 整份文件模式 | 已合入代码的阶段 review（如 M4、M5） | 报指定文件中现存的问题，不受“只报本次引入”的限制 |

整份文件模式必须明确指定文件清单；每个文件最后标为“已审”或“跳过 + 原因”，不能静默遗漏。

### 必须审查的文件

测试代码、合成 fixture 与快照**在范围内**。不少工具默认跳过 `tests/`、`fixtures/`、`snapshots/`；
本项目的主要风险之一正是“测试通过但没测到该测的逻辑”，所以必须覆盖这些文件。

### 输出格式

- 每条一个问题：`[P0–P3] 祈使句标题 — path:line`，后接一段说明触发条件、实际后果与依据。
- 依据必须可核对：代码路径、测试名与断言、或违反的规则编号（如 `T2`、`testing-policy A`）。
- **准确率优先**：不报推测性担忧、风格偏好和与规则无关的重构建议；没有问题就写“没有发现问题”，不凑数。
- 只报告，不修改代码。修复是否执行、怎样执行由人决定。
- 末尾给覆盖摘要：总文件数、已审、跳过及原因，以及未能检查的规则。

## 2. 低价值测试写法（所有测试文件适用）

M3 曾出现“只有正确用例、覆盖很小、全部通过但没测到关键失败”的情况。以下写法要作为问题报告，
除非测试旁有说明解释为何该断言已足够。

| 编号 | 写法 | 为什么有问题 |
|---|---|---|
| T1 | 失败用例只断言 `is_err()`、`panic` 或非零退出码 | 任何失败都能通过，无法区分预期拒绝与无关崩溃 |
| T2 | 只匹配错误字符串，不检查目标字节、凭据或输出 | 错误措辞对了，副作用可能仍然错误；违反 testing-policy §2 |
| T3 | 写入与回读使用同一逻辑互相证明 | 两边共享同一个错误时依然一致 |
| T4 | 快照被重新录制，但 PR 没有说明语义变化 | 快照会把错误固化成“预期” |
| T5 | 声称的保护只有正向用例，没有触发保护的反例 | 无法证明保护在需要时生效 |
| T6 | 断言的值由被测代码计算得出，而不是独立给出 | 测试只证明代码与自身一致 |
| T7 | 只检查“某个预期文件不存在”，就声称目标未改变 | 其他文件可能已写入；应比较调用前后的文件集合与字节 |
| T8 | 测试说明承诺的行为，断言并没有检查 | 读者会高估覆盖范围 |

## 3. 路径规则

按路径匹配；一个文件可同时适用通用规则 §2。表中“场景”指 testing-policy 第 3 节对应表格的行，
审查时逐行确认：要么指出覆盖它的测试名与关键断言，要么作为“未覆盖”报告。

| 路径 | 必查内容 |
|---|---|
| `crates/core/src/source.rs` | testing-policy A（源加载、ZIP、路径、读取失败） |
| `crates/reader-*/src/**`、`crates/core/src/reader.rs` | testing-policy A 的解析行与 C 的字段保留行；对应 `docs/m4-reader-proposal.md` §1、§5 的约定；未知字段必须保留或列入未承载（铁律 6）；不按正文或文件名猜类别；解析错误不回显源值；只在本地执行（铁律 3） |
| `crates/core/src/engine.rs`、`crates/core/src/governance.rs` | testing-policy B（审批、摘要、历史依据）与 D（写入、回读、回执） |
| `crates/core/src/gate.rs`、`crates/core/rules/**` | testing-policy C 的密钥行；pass 策略允许原样写入，不能误写成“目标无敏感信息” |
| `crates/writer-*/src/**`、`crates/core/src/writer.rs` | testing-policy D 与 C；embedding 无法承载时有重嵌入计划（铁律 5）；DNA 字段无法承载时显式报告（铁律 6）；不接管无关用户文件 |
| `crates/core/src/reports.rs`、`schema/**` | 报告如实区分计划、写入、回读与保存；不新增未经确认的字段或错误码 |
| `crates/cli/src/**` | 失败提示说明阶段、原因、目标状态与下一步；stdout/stderr 不泄露敏感值；未证明安全前不建议“重试即可” |
| `crates/**/tests/**`、`crates/cli/tests/fixtures/**` | §2 全部规则；只用合成数据；测试说明与断言一致 |

## 4. 执行方式与阶段 review 的附加检查

### 执行方式

- **每个 PR**：交付前由主会话启动 review agent，用改动模式跑一遍（它通过 `AGENTS.md` 读取本文），不引入额外工具。
  目前没有 CI 或 hook 自动触发，这一步是 `AGENTS.md` 交付步骤中的硬性要求，结果记在 PR 合规表的“第 1 层 review”一行。
- **阶段 review**：由用户指派阶段 review 任务触发。用整份文件模式并行跑两遍互相独立的审查，再加下面的变异测试和 §3 的场景逐行核对。
  两遍可以都用 review agent，也可以其中一遍用 OCR delegate。M4 Markdown 试点中，两遍各自的发现重合很少（见 #19），只跑一遍会漏掉相当一部分问题。
- **判定**：所有发现先由主会话逐条判定为真实、误报或重复，并给出证据，然后才交给人。人不直接处理未判定的原始发现。
- OCR 不是必需依赖；`.opencodereview/rule.json` 继续由本文派生。

### 变异测试

阶段 review 时，对该阶段的 crate 运行一次变异测试（[cargo-mutants](https://mutants.rs/)：故意改坏代码，再看测试是否失败）。
它不作为每个 PR 的门槛。

```sh
cargo mutants --package <crate> --test-package mem-adaptor-cli --test-package <crate> --timeout 240 -- --offline
```

Reader/Writer 的主要测试在 `mem-adaptor-cli` 中，所以必须用 `--test-package` 加入它。
cargo-mutants 27.1 的未变异基线只运行被变异 crate 自身的测试（可能为 0 个），据此自动设定的超时过短，
会把正常测试误报为 TIMEOUT，所以要显式给出 `--timeout`。CLI 测试套件本身是否全部通过，另以 `cargo test --workspace --offline` 确认。

每个存活的变异（代码被改坏但没有测试失败）必须分类：

- **弱断言**：有测试经过这段代码，但没有检查结果。
- **缺场景**：没有任何测试触发这条分支。
- **等价变异**：改动后行为确实不变，例如只影响日志或版本字符串；需说明理由。

变异测试只能证明已有代码是否被断言保护，**不能发现根本没写的处理**（例如 M3 的 ZIP 同名条目）。
缺失场景靠 §3 的场景逐行核对发现。

## 5. 工具入口

| 工具 | 入口 | 维护方式 |
|---|---|---|
| 本仓库委派的 review agent | 读取 `AGENTS.md`，其中一行指向本文 | 无需复制 |
| OCR | `.opencodereview/rule.json` | 由本文派生的简短规则，并指回本文对应小节；本文修改时在同一 PR 同步，用 `ocr rules check <path>` 核对 |
| Factory CI review | `.factory/skills/review-guidelines/SKILL.md` | 尚未建立 CI，暂不创建 |

OCR 的规则是“第一个匹配的路径生效”，所以 `rule.json` 中具体路径排在通配路径之前；
`include` 用来覆盖它默认跳过测试与 fixture 的行为。
