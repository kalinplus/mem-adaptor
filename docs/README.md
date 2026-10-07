# 文档索引

设计与验收文档，按「你想做什么」组织。**动手改代码前必读 [design.md](design.md) 与
[../AGENTS.md](../AGENTS.md)**（后者是铁律，前者解释铁律怎么来的）。

| 你想做什么 | 读哪份 |
|---|---|
| 控制每轮 review 的阅读量与停止点 | [review-workflow.md](review-workflow.md) — 文件/函数意图说明、Issue/PR 交接、人工 review 包与证据分类、人工放行与当前补救建议 |
| 配置或执行自动 review | [review-rules.md](review-rules.md) — 路径规则、低价值测试写法、变异测试分类与各工具入口 |
| 规定失败处理与测试验收 | [testing-policy.md](testing-policy.md) — 失败分类、副作用与凭据断言、故障注入方法及现有覆盖边界 |
| 改设计、加模块、动铁律 | [design.md](design.md) — 19 条设计决策（DEC-1~19），每条附「证据 → 理由 → 后果」 |
| 先看整体长什么样 | [diagrams.md](diagrams.md) — 框架图、一次运行的流程图、家模式图、单条记录的处置状态（Mermaid 源 + [diagrams/](diagrams/) 下的 PNG） |
| 理解内部记录为什么分必需与可选 | [canonical-record-guide.md](canonical-record-guide.md) — 两类用途、一条贯穿例子与重要能力边界，不是字段百科 |
| 理解计划、审批与回执怎样衔接 | [migration-report-guide.md](migration-report-guide.md) — 两份报告加审批凭证，正常与拒绝例子、可信边界及有限阅读入口 |
| 开始写代码、知道先做什么 | [implementation-plan.md](implementation-plan.md) — 技术栈（Rust 核心 + Python conformance）、仓库布局、核心接口、D1 的 M0–M7 里程碑与验收项 |
| 查看 M1 的字段与命名 | [schema-v0-proposal.md](schema-v0-proposal.md) — 已确认、已实现的 schema v0 方案，正本在 `schema/` |
| 查看 M3 的审批与回执链契约 | [m3-contract-proposal.md](m3-contract-proposal.md) — 已确认、已实现的元数据摘要与历史写入验证字段 |
| 查看 M4 的 Reader 契约与验收 | [m4-reader-proposal.md](m4-reader-proposal.md) — 已确认、已实现的类别保护映射、未知字段传递与输入约定 |
| 查看 M5 的 Writer 契约与验收 | [m5-writer-proposal.md](m5-writer-proposal.md) — 已确认、已实现的 OKF/UMP 映射、原生哈希与共享产物审批绑定 |
| 查看 M0–M5 review 的七项修复 | [m0-m5-review-fixes.md](m0-m5-review-fixes.md) — 已实施的检测、精度、历史身份与 Writer 产出证明修复 |
| 查看 M6 的家模式边界 | [m6-cli-proposal.md](m6-cli-proposal.md) — 两项推荐已确认，实施暂停 |
| 查看历史实现与实验记录 | [PROGRESS.md](PROGRESS.md) — 五问表、历史验证与回看资料，不再作为任务状态正本；新任务使用 GitHub Issue/PR |
| 理解「记忆到底怎么存」 | [memory-products.md](memory-products.md) — 八套实验系统的形态总览 |
| 写 Reader（新增源） | [source-memory-formats.md](source-memory-formats.md)（网页端 + harness）、[codex-memory.md](codex-memory.md)（Codex 本地落盘）、[reader-prompts.md](reader-prompts.md)（无导出通道产品的 Prompt） |
| 写 Writer（新增目标） | [design.md](design.md) 的 DEC-15（四类目标形态的写入策略）+ [step4-5-report.md](step4-5-report.md) 的逐字段矩阵 |
| 查外部项目（UMP/AIMEM/OKF/Memanto/Remnic…） | [ecosystem.md](ecosystem.md) — 身份、地址、调研分档、实测发现 |
| 要一条记忆的完整真实字段 | [step4-5-report.md](step4-5-report.md) — 五种形态并排的逐字段矩阵 |

## 文档之间的关系

```
AGENTS.md（协作规则 + 铁律 + 模块边界）
    │
design.md（设计决策 + 为什么，是推导）
    │
    ├── review-workflow.md        小功能交付、Issue/PR 状态与人工 review 停止规则
    ├── review-rules.md           自动 review 规则正本，工具入口由它派生
    ├── testing-policy.md        失败行为与测试规范，不代表所有要求已实现
    ├── diagrams.md               design.md 的图示版，冲突时以 design.md 为准
    ├── implementation-plan.md    design.md 的落地计划：语言、布局、接口、里程碑
    │   ├── schema-v0-proposal.md 已确认、已实现的 M1 字段与命名
    │   ├── m3-contract-proposal.md 已确认、已实现的 M3 审批与回执字段补充
    │   ├── m4-reader-proposal.md 已确认、已实现的 M4 Reader 与字段传递方案
    │   └── m5-writer-proposal.md 已确认、已实现的 M5 Writer 与目标保护
    ├── memory-products.md        记忆产品侧：八套系统怎么存
    ├── source-memory-formats.md  源侧：网页端导出 + harness 落盘
    ├── codex-memory.md           源侧：Codex 本地记忆
    ├── reader-prompts.md         源侧：没有导出通道时怎么抽
    ├── ecosystem.md              外部项目：谁是谁、该不该调研
    ├── step4-5-report.md         实验报告：真实落盘字段 + 复现命令
    └── PROGRESS.md               历史资料，不再追加任务进度
```

**维护约定**：接入新系统时加一列 + 一节，不写 n(n−1) 份配对走查
（配对数是平方增长）。新的外部项目先落 [ecosystem.md](ecosystem.md) 的地址清单，
确认要调研再补「实测发现」小节。
