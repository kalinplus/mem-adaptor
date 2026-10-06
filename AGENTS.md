# mem-adaptor

开源记忆迁移工具链：把记忆从真实平台（ChatGPT 导出、Claude projects、Gemini Takeout、Mem0/MCP 服务器、Obsidian）读出，归一、清洗、去重，按目标平台形状写入，每步产出机器可验证的凭证。

定位是「搬家的施工队」，不是格式规范制定者，也不是又一个记忆存储。两种用法：直迁（源 → 目标，一次性）与家模式（用户在现有格式里选一个「家」，默认 OKF 目录，其余工具是卫星，定期汇总进家并对账；MVP 只做卫星 → 家）。见 [docs/design.md](docs/design.md) DEC-19。

## 协作规则（每轮必读）

完整说明见 [docs/review-workflow.md](docs/review-workflow.md)。本节约束交付节奏，不改变下面的架构铁律。

### 代码意图说明

- 每个手写源文件必须有顶部说明：职责、调用链位置和重要边界，包括测试文件与辅助脚本。
- 每个函数/方法必须有简短意图说明，包括私有 helper、trait 方法和测试函数。简单函数一句话；复杂函数说明输入输出的含义、关键约束、主要失败条件与副作用。
- Rust 用 `//!` 写模块说明、`///` 或 `//` 写函数说明；注释用英文，解释目的与原因，不逐行翻译代码。修改实现时同步更新说明。
- JSON 不添加非法注释；schema、fixtures、第三方原样复制及生成文件采用格式允许的说明或配套 README。
- 现有代码按用户授权的小范围逐步补说明，不自行批量改全仓库；说明与实现不一致时明确指出，不把行为修复混入仅注释任务。

### 失败处理与测试

- 修改源加载、解析、校验、审批、历史保护、写入、回读或报告保存路径前，必须阅读 [docs/testing-policy.md](docs/testing-policy.md)，将相关行为要求逐条纳入本轮验收条件。
- 执行依据缺失、失效或不一致时，拒绝相关写入，不猜测、不绕过；不静默吞掉错误，不把跳过、部分完成或无法验证表示为完整成功。
- 区分写入前与写入后失败。只有有证据时才能声明目标未改变；无法确认时保守报告可能部分完成。不承诺未实现的回滚、恢复、安全重试或历史保护。
- 普通运行错误通过 `Result` 传播，在关键边界补上下文，由 CLI 统一呈现失败阶段、原因、目标状态与下一步；错误和日志不得泄露敏感值，不把程序缺陷伪装成业务跳过。
- 行为修改必须同步相关说明与故障测试。测试核对返回结果、目标实际内容、报告/凭证和脱敏输出，不只判断退出码或匹配错误字符串。
- 使用合成数据、隔离临时目录和确定性的故障注入；不得为测试耗尽真实磁盘、破坏真实记忆或终止无关进程。
- 交付逐条提供实际命令、断言与证据；未实现、未运行或未验证的要求必须注明。本文规定测试要求，不代表现有实现已满足全部要求，不自行扩大当前任务。

### 任务来源与授权

- 正式任务采用 `/issue-workflow`：一个任务 = 一个 GitHub Issue = 一个 worktree = 一个分支 = 一个 PR。Issue 存需求与当前交接，PR 存候选实现。
- GitHub 是任务状态的唯一正本。现有 `docs/PROGRESS.md` 和里程碑方案保留为历史资料，不再追加任务进度或新 specs；不创建本地 review 状态账本。会话 TODO 只列本轮工作，不是任务状态正本。
- 每轮只执行一个用户明确指派、边界和验收条件已确认的任务。`status:ready` 仅表示可领取，不授权 agent 自行领取其他任务；大里程碑先拆成可独立 review 的小任务。
- 开始前记录需求快照。被“继续/下一步”唤起时，先同步指定 Issue 正文/评论与关联 PR 的真实状态；没有明确的新任务授权，只同步、讨论或澄清，不自行推进。
- 启用工作流不等于授权远端写入。创建 Issue、初始提交和发布须有相应用户授权；后续按用户明确指派及工作流约定执行。提交基线只保存候选实现，不代表人工验收。

### 交付后必须停止

- 方案批准只授权当前任务的实现。测试通过、AI review 通过、用户沉默和旧方案批准，都不能替代人工验收。
- 交付前加载 `pr-delivery`，逐条核对 Issue 验收条件；交付草稿 PR，把 Issue 标为 `status:review`，更新交接与关联实现，提供文件 → 关键函数 → 测试的阅读顺序和真实验证结果。
- 交付动作完成后必须结束当前回复，等待用户 review；不得自动领取、探索或实现下一任务，也不得启动为下一任务工作的子 agent 或后台任务。
- 用户要求修正只授权当前范围内的相应修正，修正并验证后再次交付等待；扩大范围或改变设计必须确认。
- 合并或关闭需要用户确认。成功完成以 PR 合入并通过 `Closes #N` 关闭 Issue 为准；放弃关闭必须说明原因，不视为成功。
- 当前任务验收与下一任务授权是两件事：当前 PR 合并不自动授权下一个 Issue，用户必须明确指派下一任务。当前 M6 仍暂停。

## 铁律（不可违反的架构决策）

以下决策经过完整调研核实，**每条结论的推导过程与证据见 [docs/design.md](docs/design.md)**，
任何改动需用户显式确认，不接受"顺手改进"：

1. **不自建交换格式。** canonical model 只是内部脚手架，永不对外发布为规范、不做对外序列化格式。对外只暴露映射器：UMP JSON、AIMEM bundle、MIF bundle、OKF Markdown 信封、Mem0 API 形状。格式层的名字通胀（AMP 五义、MIF 改名）是前车之鉴。
2. **默认 dry-run。** 任何真实写盘前必须先产出迁移报告等人确认；CLI 的默认行为是 dry-run，真实写盘是显式动作。
3. **抽取默认本地。** 解析、归一、去重默认全部本地执行；调用远程模型必须显式 opt-in，且调用事实写入迁移报告。
4. **scope/owner 不是安全边界。** `owner` 是客户端自声明的（A2M 规范明示）；网络场景必须由认证主体推导，审批钩子不得信任自声明 owner。
5. **禁止静默重嵌入。** embedding 模型名+维度必须随记录走；目标不支持时，迁移报告必须给出结构化重嵌入计划（哪些条、用什么模型、质量影响预估）。
6. **禁止静默衰减。** AIMEM DNA 类语义（不可静默丢弃的字段）在映射中无法承载时，必须显式列入报告，不得悄悄丢字段。
7. **冲突不自动裁决。** 语义去重只做候选聚类，输出冲突清单给人裁决；裁决结果写回复合键。不写自动合并逻辑。
8. **Conformance 套件不 import 被测实现。** 套件独立于实现（学 A2M），适配器作者不 merge 也能自证合规。

## 模块边界

```
Reader 插件（源适配器，社区可贡献）：解析 + 归一，交出源记录 + canonical 记录 + 未承载字段清单
  → 核心引擎（全部本地执行）：
    校验 canonical → 密钥/PII 出站闸门 → 去重/冲突聚类 → dry-run 闸门（计划报告）→ 人工确认/审批钩子
  → Writer 插件（目标映射器，可多目标同时导出）：写入 → 回读验证（回执报告）

上一次的回执报告是下一次运行的输入（幂等、防复活、裁决复用），不另建状态账本。

横切：治理接口（approval receipt / 快照 / 审计链 / 导出授权）、conformance 套件、CLI 优先 MCP 为辅
```

- **Reader/Writer 是插件**，核心引擎不 import 具体适配器；适配器通过注册表接入。
- **治理是可插拔后端**：approval receipt 接 Panella 式哈希链凭证，快照接 Memoria 式 CoW，本仓库不自建治理机制，只留接口。
- **迁移报告是一等 artifact**：带 schema 的机器可验证文件，同时是收据、合规凭据、用户可读的解释。报告 schema 是本仓库的核心资产。

## MVP 范围与节奏

- **D1**：三个 Reader（ChatGPT 数据导出、Claude projects/conversations、本地 Markdown/Obsidian）+ canonical model + 迁移报告 schema + 默认 dry-run + 密钥正则检测（检测常开、命中必报；处置默认放行，`init`/首次运行让用户选放行或拦截，策略记入报告）。Writer 只做 UMP JSON 和 Markdown/OKF 两种；OKF 目录同时是默认的家，所以 Markdown Reader 要能读回 OKF 家目录。
- **D2–3**：写入侧 Writer（Mem0 API 形状——兼容 Mem0 即兼容 PolarDB/腾讯云复刻；basic-memory Markdown 目录）+ PII 出站检查（接现成 NER：高危 PII 与密钥同一套策略，默认放行、可设为拦截；一般个人事实只列入报告）+ 导出审批钩子。
- **长期**：语义级冲突清单 → 撤回闭环（记忆版本链 + 下游传播记录）。

不做的事：交换格式规范、自建中心存储/档案格式、自动冲突裁决、SaaS 化、自建治理账本。

实现：核心（引擎、Reader、Writer、CLI）用 Rust，conformance 执行器用 Python；schema 以 `schema/` 下手写的 JSON Schema 为正本。仓库布局、核心接口与 D1 里程碑见 [docs/implementation-plan.md](docs/implementation-plan.md)。

## 术语与外部项目

UMP、A2M、AIMEM、MIF、OKF、memcommons、Remnic、Panella、Memoria、PAM 等代号的完整身份、地址、核实状态见 [docs/ecosystem.md](docs/ecosystem.md)。涉及这些项目时先读该文档，地址以文档为准。

UMP 同时做协议 + 参考实现 + 导入器，存在吞并重叠风险。

## 记忆形态参考

**全部文档的入口见 [docs/README.md](docs/README.md)（按「你想做什么」组织）。**

动手实验过的八套系统（server-memory / basic-memory / mem0 / Graphiti / claude-mem / agentmemory / Memoria / Panella）的落盘形态、身份字段、冲突处理见 [docs/memory-products.md](docs/memory-products.md)；源侧（ChatGPT / Claude / Gemini 导出、Claude Code / Codex 本地记忆）见 [docs/source-memory-formats.md](docs/source-memory-formats.md) 与 [docs/codex-memory.md](docs/codex-memory.md)，无导出通道产品的抽取方式见 [docs/reader-prompts.md](docs/reader-prompts.md)。写 Reader/Writer 前先读对应文档，不要凭记忆假设某家的字段。
