# 当前进展：动手摸记忆系统

目的：对「记忆到底怎么存在」建立体感——跑起来、喂输入、直接打开落盘文件看格式。
不看论文。有了体感再回头看 UMP 等项目做了什么。

产出目标：一份跨系统的字段对照表，作为迁移报告 schema 的第一版字段来源。

> **文档地图**：全部文档的入口见 [README.md](README.md)（按「你想做什么」组织）。**改设计前读 [design.md](design.md)**（19 条设计决策，每条附证据与理由）。想先看「八套系统分别是什么形态、机制是什么」，读 [memory-products.md](memory-products.md)（总览，含名字对照与五条横向规律）；要每条记忆的完整真实字段，读 [step4-5-report.md](step4-5-report.md)；源侧（网页端导出 + 本地 harness）读 [source-memory-formats.md](source-memory-formats.md)、[codex-memory.md](codex-memory.md)；无导出通道产品怎么抽记忆读 [reader-prompts.md](reader-prompts.md)；外部项目（UMP / OMPI / AIMEM / MIF / OKF / Memanto / Remnic / MacPaw portable-memory 等）的身份、地址与**调研优先级分档**读 [ecosystem.md](ecosystem.md)。

## D1 实现 TODO（2026-10-05）

按 [implementation-plan.md](implementation-plan.md) 的 walking skeleton 顺序推进：

- [x] **M0 准备**：初始化本地 Git；补 `target/` 忽略和合成 JSONL fixtures 例外；检查根 `.mcp.json`，未发现凭据模式；建立 7 个 Rust crate 和 CLI。
- [x] **M1 schema v0**：[字段方案](schema-v0-proposal.md) 已由用户确认；5 份 schema、Rust 类型、6 个有效向量、38 个无效变体和三类一致性测试已完成。
- [x] **M2 最小链路**：Markdown Reader → OKF Writer；`plan → apply → 审批 → 回读 → 回执`；dry-run 不动目标、源变更拒写、YAML 未知键/顺序往返均已验证。
- [x] **M3 引擎切片**：ZIP 路径检查、源认领清单、schema/字段覆盖校验、密钥闸门、精确去重、旧回执防复活和完整计划报告。
- [x] **M4 三个 Reader**：DNA 类映射与 `source_extra` 已确认；Markdown/OKF 家、ChatGPT、Claude；
  合成目录/ZIP fixtures、源清单、逐条字段关联、家往返与三份报告快照已验证。
- [x] **M5 两个 Writer**：kind/title、缺失创建时刻与完整目标保护均已确认；
  完整 OKF 与 UMP（Universal Memory Protocol，通用记忆协议）Writer、原生快照、
  官方 schema 校验、共享产物审批绑定与历史更新已验收，见 [M5 契约](m5-writer-proposal.md)。
- [ ] **M6 CLI 与配置**：`init`、家模式、首次策略选择、非交互行为、裁决和退出码；
  [M6 方案](m6-cli-proposal.md) 的跨卫星对账与显式卫星身份已确认，按用户要求暂停实施。
- [ ] **M7 独立验收**：Python conformance 执行器；真实数据只读、临时输出、不进库，真导出包尚未提供。

**M0 验证记录**：`cargo build`、`cargo fmt --all -- --check`、`cargo test --workspace`
（4 个 CLI 集成测试）、`cargo clippy --workspace --all-targets -- -D warnings` 全部通过。
用 Cargo metadata 核对依赖方向；验证敏感/运行时路径被 Git 忽略、合成 JSONL 可纳入版本控制；
在临时目录验证版本、帮助、无参数和未实现命令均不改变目录字节。未提交、未推送，未读取真实记忆。

**M1 验证记录**：7 个 schema 集成测试通过，覆盖正本自身的 draft 2020-12 合法性、
有效向量无损往返、Rust 构造的最小/完整示例、无效向量拒绝、逐个删除必需字段、
全部处置/原因/验证结果变体、只有模型与维度的 embedding。
再次运行 `cargo build`、`cargo test --workspace`（共 11 个集成测试）、
`cargo clippy --workspace --all-targets -- -D warnings`、格式检查与依赖边界检查，全部通过。
M1 向量中的哈希只验证形状，不声称已做内容哈希或计划摘要重算。

**M2 验证记录**：21 个测试（含 WriteToken 不可外部构造的编译失败测试）通过；
`cargo build`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 全绿。
端到端测试固定了目标字节不变、审批绑定摘要、源变更拒写、回执 schema 和回读一致、
非交互无显式确认不写、报告不能落到源/目标里。日志覆盖 `[S1]`–`[S9]`。
`serde-saphyr` 1.3.0 配合 `serde_json/preserve_order` 保留未知键、嵌套类型与键顺序；
`serde_jcs` 0.1 的 UTF-16 排序不符合 RFC 8785，升级到 0.2 后专项测试通过。
frontmatter 在最小 Reader 中只登记为未承载，正文逐字节保留；完整字段落位在 M4/M5。
M2 验收时 M3–M7 尚未完成，因此 CLI 限合成数据；M3 后该限制仍保留至完整适配器与验收完成。

**M3 验证记录**：[契约补充](m3-contract-proposal.md) 已确认并实现：

- ZIP 整包预检后才解压，拒绝绝对路径、`..`、Windows 路径、重复路径和符号链接；
  只使用临时目录，源定位仍为原 ZIP。尚无资源配额，不允许不可信大包。
- 引擎校验 canonical schema、id 派生、正文哈希和向量长度；漏报源字段进入 unmapped/anomalies。
- Gitleaks 六条签名正则加本地密码赋值规则常开；pass/block/规则白名单均报告；
  假密钥不进入计划、回执、stdout、stderr 或捕获的日志文件，未知源字段同样扫描。
- `record_hash` 绑定完整 canonical；覆盖前的 `prior_write` 也进入预测摘要。
  Python 标准库对实际、无数字/非 ASCII 键的摘要输入独立重算通过，不冒充 M7 完整执行器。
- 重复运行不改目标；稳定 id 原子更新；手删目标、空写入、重复引用和暂时源缺失均不丢防复活依据；
  双侧变化/无历史依据不自动覆盖；旧裁决复用但不能绕过 block；unverifiable 历史要求人工确认。
- schema 向量扩到 44 个无效变体，仍为语言中立数据。Unix 新报告权限为 `0600`。
- 当前 44 个测试（含编译失败 doctest）、workspace 构建、格式检查和 Clippy 全绿。
  M4–M7、首次持久化配置和 PII 检测仍未完成；未提交、未推送，未读取真实记忆。

**M4 验证记录**：[Reader 方案](m4-reader-proposal.md) 两项推荐均已确认并实现。

- `source_extra` 只携带逐条未知 metadata，完整记录哈希与闸门均覆盖它；
  计划/回执不内联这些值。字段映射增加可选规则名，源清单增加删除计数。
- 映射与未承载字段绑定 `canonical_id`，一条的映射不能掩盖另一条遗漏；
  不同源的相同原始 id 也不会串用密钥命中、引用标记或字段清单。
- 三个 Reader 已注册到 CLI。Markdown 正文字节、Claude Code 会话来源/修改时间、
  Prompt 坏行/日期/未知分类、Claude 新旧优先级、整块旧记忆、项目文档和指令均有合成验收。
- 两家的会话只登记；未知来源的空会话数组不猜平台。ChatGPT 的合成层、溯源和未经核实的指令字段显式列为不可得。
- 目录和 ZIP 经 `plan → apply` 验收，写出的家读回能恢复原身份、scope、分类与完整 metadata 哈希。
- `crates/cli/tests/fixtures/m4/snapshots/` 三份计划快照已通读并逐项核对，
  固定运行 id、时刻与路径后重算摘要；不是可直接批准执行的真实计划，也不是 M7 的独立执行器。
- 原源 metadata 与家信封的同名字段值冲突时拒绝读取，不自动覆盖；额外无冲突信封字段仍保留。
- 当前 **62 个测试**（含 doctest）、workspace 构建、格式检查、Clippy 全通过；
  schema 向量扩为 50 个无效变体。
- M5–M7、配置持久化、真实导出验证与 PII 检测未完成。仍仅使用合成数据，未提交、未推送。

**M5 验证记录**：已确认的映射与目标保护契约全部落地。

- OKF 的 title/sources/generated/tags、scope 索引和追加日志已实现；
  不生成 description/verified，不伪造作者。原源 metadata 与正文完整保留；
  向量移除进入 transformed 与重嵌入计划，重复迁移不会循环更新。
- UMP 写 `records.ump.json` 数组，保守 kind 映射、可逆 id、私有 opaque owner；
  原身份/分类/时间/向量/metadata 可回读。缺失源创建时刻时明确标为
  `target_migration`，更新保留首次目标创建时刻及其来源标记。
- 官方 schema 与 Apache-2.0 license 原样 vendored，离线验证，不依赖 `lab/upstream`。
  同名重复记录、无效数组、符号链接、未处理的 `consent.redact` 均不会被静默接受。
- 原生 `target_hash`、共享 `artifacts`、`target_map` 已进入报告与审批；
  WriteToken 对 Writer 实际读取的字节再次核对，覆盖引擎检查后发生的变更。
  原生展示标题/创建时刻、索引/日志/数组被改动时不自动覆盖。
- 16 个 Writer 测试包括官方 schema、未知字段/向量往返、删除防复活、源更新、
  审批后变更、Writer 入口竞态、无关产物保留及四份原生产物快照；
  M4 的三份报告快照已按新契约更新，源字段关联保持不变。
- 空写入回执保留共享产物的此前写入依据，受阻运行不洗白用户修改；
  连续两次历史迁移的回归测试覆盖此边界。
- 当前 **78 个测试**（含 doctest）、workspace 构建、格式检查、Clippy、
  依赖边界和 fixtures Git 可见性检查全部通过；schema 无效变体增至 60 个。
- 写入是单文件原子替换，不是跨文件事务，不提供跨进程锁。M6–M7、
  真实导出验证与 PII 检测未完成。未读取真实记忆，未提交、未推送。

**M0–M5 review 修复（2026-10-06）**：用户授权后修复七项已复现问题，
包括键名密钥、不安全整数、重复代表/自身历史、写后产物证明、受管预检和已有重嵌入计划。
新增 `Finding.key_hash` 与 `duplicate_write` 契约；Writer 返回实际输出证明。
当前 **87 个测试**、离线构建、格式及 Clippy 全通过，schema 无效变体增至 67 个；
M4/M5 快照无需重录。详见 [修复记录](m0-m5-review-fixes.md)。
M6 仍暂停，未读取真实记忆，未提交、未推送。

## 五问模板（每体验一个产品，填一列）

1. **怎么作用到 agent 上**：接线形态（谁驱动写入、进程监听什么）、**何时触发，有无保证**（模型自觉=无保证，钩子/代码=有保证。实证：本 lab 会话里 basic-memory 挂载着但全程零调用，不触发是 A 类常态）、迁移时从哪读。
2. **记忆存储形式/格式**：哪种形态、什么物理格式；**真相在哪份文件/库**；**身份字段是什么**（user/scope 之类，跨系统对不上就是静默丢信息）。
3. **记忆如何新增/变化**：谁写（模型自觉/钩子/人改文件）、矛盾与更新怎么处理、时间在不在语义层。
4. **怎么被取回**：检索依赖什么（子串/倒排/向量/链接）、跨语言表现、噪声控制——这是 Writer 的验收标准：写进去的东西要能用目标自己的检索找回来。
5. **边界：什么会出去、什么拦不住**：哪些调用出网（LLM/embedding/遥测）、PII/密钥有没有拦截——迁移报告的合规字段来源。

| 五问 | Step 0 server-memory | Step 1 basic-memory | Step 2 mem0 | Step 3 Graphiti | Step 4 claude-mem | Step 4b agentmemory | Step 5 Memoria | Step 5b Panella |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 接线 | A 类被动工具（MCP stdio，无状态进程）；触发=宿主 LLM 自觉，无保证；读 jsonl | A 类被动工具（MCP stdio，CLI 同体）；触发=模型自觉，可选 Claude Code 插件 hooks 补保证；真相=Markdown 目录 | 进程内库（另有 OpenMemory MCP / 平台 REST 面）；触发=接入方代码显式调 add()，默认形态下有保证；若是 MCP 形态则同 A 类自觉；真相=db | 进程内库，另带 FastAPI server（`server/`）和一个 MCP server；触发=接入方代码显式调 add_episode()（本次直接调）；真相=Neo4j 容器，无人类可读落盘文件 | 旁路钩子（8 事件：Setup/SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/PostToolUseFailure/Stop/SessionEnd），触发有保证；真相=自建 SQLite | 旁路钩子（12 事件）+ 插件自带 `.mcp.json` 自动接 MCP（54 工具）；真相=iii state store | 常驻服务（HTTP 8100）+ CLI 做 MCP bridge（25 工具）；触发=接入方显式调用 | 常驻 store(8000)+facade(8001)，MCP HTTP；写路径只有 propose |
| 2 存储 | jsonl 裸图谱：type/name/entityType/observations 四字段；无身份字段、无时间戳 | Markdown+frontmatter（title/permalink/tags），关系=正文 `[[链接]]`，SQLite 索引可再生；身份=permalink，无 user 维度 | 事实级英文短句（LLM 转述）+ 2048 维向量，payload{user_id, run_id, data, hash, created_at}；有 user/run；**无 embedding 模型名** | 三元组 `Entity-RELATES_TO->Entity`，边的 fact 是 LLM 转述的**中文**短句，节点和边各存一份 embedding；**每条边三组时间** valid_at/invalid_at/expired_at；身份=group_id；有无 `expired_at` 是它和别家最大的字段差；**embedding 模型名同样不随记录走** | `claude-mem.db`（SQLite：observations/sdk_sessions/user_prompts/tool_uses）+ Chroma；身份=**project**（git 仓库名或目录名），**无 user 维度** | iii state store（`data/`，无人类可读格式）；身份=session/memory id（`sessionIds` 关联） | MatrixOne 容器 volume，**无人类可读文件**；身份=**user_id**（per-user DB + `mem_user_registry`）；带 `trust_tier`/`initial_confidence` | Compose volume `panella-store`（`sqlite_vec.db`）；身份=`tenant_id`/`wing`/`room`；drawer 带 `content_sha256`/`valid_from`/`approval_ref` |
| 3 变化 | 全靠调用方 CRUD；矛盾并存，无失效机制，时间须自己编码进文本 | write 拒绝覆盖、append 无幂等；矛盾并存但索引 created_at 可排序（写入时间≠事实时间） | LLM 判 ADD/UPDATE/DELETE；实测矛盾句判了 **ADD 不是 UPDATE**，两条并存；还会幻觉注入当前时间 | LLM 判重复/矛盾，**矛盾真的写进语义层**：R5 把 R1 三条边全部标 invalid_at=R5 的 valid_at。但裁决是「标失效」不是「合并」：R1 的三条同义转述边并存、一起失效，R5 的新表述还并进了 R1 旧边 | 每次工具调用压成一条 observation（observer LLM 必须守 `<observation>` XML）；`content_hash` 字节级去重；近似去重 opt-in | observation→memory 两层，memory 带 `version`/`supersedes`/`strength`；consolidation 默认关 | CRUD + `correct` + **snapshot/branch/checkout/rollback/merge**；矛盾两条**并存不裁决**，答案是回滚 | **default-deny**：写=提案→人批准→落库；「已批准」与「已落库」是两步 |
| 4 检索 | 纯子串匹配，无 LLM；query 换词质量决定召回 | FTS5+本地 bge-small 混合；中文 query 在英文模型下翻车（命中错笔记） | 纯向量余弦（embedding-3 多语），中文正确；分数 0.3–0.5 无默认阈值，噪声全量返回 | BM25+向量混合 + LLM rerank；中文 query（「数据库」）正常；**已失效的边照样全量返回**——「事实在 valid_at~invalid_at 之间有效」是写给下游 LLM 的提示词，不在 query 里过滤 | FTS + Chroma 向量；MCP 3 工具（search/timeline/get_observations） | 默认 **BM25**（零 LLM）；开 `EMBEDDING_PROVIDER` 才有向量 | 向量+全文混合；中文 query「数据库」命中 score 0.633 | 本地 embedding（模型烘进镜像）+ wing/room 过滤 |
| 5 边界 | 全本地零外呼；API key 原样落盘 | 全本地（首跑联网下 64MB 模型）；key 原样落盘 | 抽取+embedding 发智谱；PostHog 遥测默认开；key 被转述强化入库 | 每集多次 LLM 调用（抽实体+抽边+两级去重），rerank 也走 LLM，全部出网；key 直接变成图谱实体 `API 密钥 sk-test-12345`（不再是字符串字段，是节点） | **每次工具调用**都调 observer 模型（默认出网，本次 glm-5.3）；密钥拦截未测 | **默认零 LLM、无 key**——四套里唯一默认不出网；密钥拦截未测 | embedding/LLM 按 `.env`（本次智谱 embedding-3/2048 + glm-5.3）；密钥拦截未测 | 默认无 BYOK 也能跑，embedding 烘进镜像；密钥拦截未测 |

四列横评定型的事实：**PII (Personally Identifiable Information，个人可识别信息)拦截四连零**（源系统不设卡是常态，闸门只能在迁移管道里）；**矛盾处理只有 Graphiti 进了语义层**——别家是两条并存，它是标 invalid_at 并把「何时开始不真」写进字段。但「标失效」不等于裁决：它不说哪条对，选择权仍然在读到日期的人（或下游 LLM）手里。

**八列（含 Step 4/5 四套）横向补充**：五问表的第 2 问（身份字段）是迁移丢信息的重灾区——mem0 有 `user_id`、claude-mem 只有 `project`、Memoria 是 per-user 分库、Panella 是 `tenant/wing/room` 三段式，**没有两家能直接对上**。另外**四套全都不记录 embedding 模型名**（Memoria 只把维度 `VECF32(2048)` 写进 schema），铁律 5（禁止静默重嵌入）在现实里是普遍缺失。

**逐字段矩阵**（每条记忆的全部字段 × 五种形态并排，含「怎么推任意 A→B 迁移计划」的三条规则）见 [step4-5-report.md](step4-5-report.md)。设计原则：**接入新系统加一列 + 一节，不写 n(n−1) 份配对走查**——配对数是平方增长，且每加一个系统就要补 n 份。

## 心智模型

**记忆的四种形态**（各家差异本质是存了哪几种、用什么结构）：原文 / 抽取的事实 / 结构化图谱 / 程序性记忆。迁移是这些形态间的有损转换。

**接线形态**（Q1 的理论版）：

| 形态 | 谁驱动 | 例子 | 迁移时读什么 |
|---|---|---|---|
| A. 被动工具服务（MCP stdio） | client 的 LLM 自觉决定读写，无触发保证 | server-memory、basic-memory | 直接读落盘文件 |
| B. 旁路钩子（挂 harness 事件） | 宿主生命周期，触发有保证 | claude-mem | 自建 SQLite/索引，私有格式最难 |
| C. 常驻服务（监听端口） | 任何 API 调用方 | Remnic daemon、Memoria | 走它的 API/导出 |

**MCP 一段话**：MCP 是宿主↔server 的运行时协议，模型不直接说 MCP——宿主把工具 schema 翻成模型 API 的 tools，模型的 tool_calls 由宿主翻回 JSON-RPC。原语有三种（tools/resources/prompts），工具只是其一。N+M 算术：模型面前工具数不变，变少的是每个工具的接入成本（手写胶水→加配置）。对迁移项目的意义：MCP 是运行时协议不是数据格式，搬的是落盘形态，MCP 只是 C 类系统的一条读取通道。

## 实验输入（四个系统喂同一段 5 轮对话）

```
R1 user: 我们项目最近把 MongoDB 换成了 PostgreSQL，因为事务要求变高了。
R2 assistant: 好的，已记录。
R3 user: 我个人一直用 Obsidian 记笔记，最喜欢的编辑器是 Neovim。
R4 user: 对了，我的 API 密钥是 sk-test-12345，帮我记住方便下次用。
R5 user: 其实我们又迁回 MongoDB 了，Postgres 运维太麻烦。
```

R1/R3=基础写入，R4=敏感信息（测拦截，预期没有），R5=矛盾（测更新/并存/失效）。

## 实验矩阵

| Step | 系统 | 形态 | 状态 |
|---|---|---|---|
| 0 | `@modelcontextprotocol/server-memory` | 裸图谱 | ✅ |
| 1 | `basic-memory` | Markdown+frontmatter | ✅ |
| 2 | `mem0ai` | 抽取事实（向量+图+KV） | ✅ |
| 3 | Graphiti | 双时序知识图谱 | ✅ |
| 4 | claude-mem / agentmemory | 钩子自动写入 | ✅ |
| 5 | Memoria / Panella | 治理（快照回滚 / receipt） | ✅ |
| 6 | 手动迁移：ChatGPT/Claude 导出 vs Obsidian 字段对照 | — | 待 |

## 已完成实验记录

### Step 0：server-memory（v0.6.3）——裸图谱

怎么跑：`lab/step0-server-memory/`，`node node_modules/@modelcontextprotocol/server-memory/dist/index.js` + `MEMORY_FILE_PATH`（必须绝对路径，否则写进包目录）。用 `lab/mcp_drive.py` 喂 `requests.json`——我手动扮演抽取器把对话拆成 entity/observation。Live 版 `lab/step0-live/sim_client.py`（手搓 ~150 行 MCP client，你说话→GLM 选工具→转发→回传，每跳打印）。

落盘 `memory.jsonl` 全文（无时间戳、无 schema 版本、无 provenance）：

```jsonl
{"type":"entity","name":"Kal","entityType":"user","observations":["Uses Obsidian for personal notes (R3)","Favorite editor is Neovim (R3)","API key is sk-test-12345, remember for next time (R4)"]}
{"type":"entity","name":"project-db","entityType":"component","observations":["Migrated from MongoDB to PostgreSQL because transaction requirements got higher (R1)","Migrated back to MongoDB because PostgreSQL ops was too heavy (R5)"]}
```

表格装不下的洞察：

1. **抽取是调用方的活。** 9 个工具全是 CRUD，对话怎么拆成实体/观察值由 client LLM 决定，同一句每次拆得可以不同。迁移视角：schema 只有四字段，读它容易，但产物已和图结构纠缠。
2. **server 里没有任何大模型。** searchNodes 就是实体三字段的 toLowerCase().includes()。live 里 glm 先搜「数据库」扑空（observation 里没这三个字）、换搜 "project" 才命中——字面检索的语义鸿沟靠 LLM 重试救。
3. Live 两轮更有价值的观察：glm-4-flash 连续 4 次参数形状错→放弃写入、直接复述用户的话假装记住了，**工具失败无降级路径、用户无感知**；检索为空时 LLM 拿对话上下文冒充记忆，用户分不清「检索到的」和「猜的」。抽取可靠性=模型能力问题。
4. jsonl 每次改动全量重写、无文件锁，两个宿主同指一个文件会互相覆盖。

自己动手：`cd lab/step0-server-memory && python3 ../mcp_drive.py "node node_modules/@modelcontextprotocol/server-memory/dist/index.js" requests.json`（改 requests.json 里的 arguments 喂自己的内容）；矛盾实验=add_observations 追加矛盾事实再 search_nodes。

### Step 1：basic-memory（v0.22.1）——Markdown + frontmatter + 可再生索引

怎么跑：`lab/step1-basic-memory/`，venv 里 `basic-memory mcp --project memlab`（project 注册表在 `~/.basic-memory/config.json`，指向 `memory-dir/`），同一个 `mcp_drive.py` 喂同一段对话。**已接入 pi**：repo 根 `.mcp.json` 有 basic-memory 条目，重启 pi 直接说「记到 basic-memory」。

落盘 `memory-dir/Kal.md`（文件即真相，`~/.basic-memory/memory.db` 是派生索引，删了 `reset --reindex` 重建；图谱不在工具里——`[[wiki链接]]` 写在正文，索引时解析进 relation 表）：

```markdown
---
type: person
permalink: memlab/kal
tags:
- user-profile
---

# Kal

Personal profile gathered from conversations.

- Uses Obsidian for personal notes (R3)
- Favorite editor is Neovim (R3)
- API key is sk-test-12345, remember for next time (R4)
```

表格装不下的洞察：

1. **图结构被降维成文本约定，由索引层再升维回来。** 正文写「- 相关笔记：[[Kal]]」，relation 表自动出现关系，中文语境也能推断 relation_type。工具面是笔记 CRUD（23 个），比 entity CRUD 高一级。
2. **文件即真相 + 索引可再生 = A 类里对迁移最友好的形态。** 可 git diff、人读人改；治理思路是「索引坏了就重建」（doctor/orphans/reset），不是「文件服从数据库」。它把你当文件的所有者、把自己当索引的租客。
3. **写入有幂等保护，append 没有。** write_note 对已存在笔记拒绝，但返回结构化的修复选项表教 LLM 下一步用 edit_note/overwrite——错误信息是给模型读的（fastembed 报错塞整页 troubleshooting 同理）。edit_note append 重复调用就重复追加。
4. 坑：`reset` 交互式确认脚本里要 `echo y |`；首次 search 慢（联网下 64MB 模型，不挂代理 `[Errno 60]` 超时）不是死锁。

自己动手：`cd lab/step1-basic-memory && python3 ../mcp_drive.py ".venv/bin/basic-memory mcp --project memlab" requests.json`（改 arguments 喂自己的笔记）；矛盾实验后 `sqlite3 ~/.basic-memory/memory.db "select title,created_at,updated_at from entity;"` 看索引时间≠事实时间。

### Step 2：mem0（OSS v2.2.1）——抽取事实：抽取第一次不在我手里

主页 https://mem0.ai/ 是托管平台（YC 系，卖点是 Memory Compression Engine、LoCoMo benchmark、SOC2 治理）；实验用开源库 `mem0ai`（MIT，进程内库）。

怎么跑：`lab/step2-mem0/`，`Memory.from_config` 三件套全指智谱（openai provider + `openai_base_url`：llm=glm-4.6 抽取，embedder=embedding-3/2048 维，vector_store=qdrant 本地模式 `path=`，history_db_path=本地 SQLite），key 从 repo 根 .env 读。注意下面这批事实是 glm-4.6 时代的产物，脚本现已统一切到 glm-5.3。`feed.py` 逐轮 add 同一段对话，`search.py` 检索。**已接入 pi**：手写最小 stdio 包装 `mcp_server.py`（pip 包不带 MCP；四工具 memory_add/search/all/history，遥测已关），`.mcp.json` 有 mem0 条目——重启 pi 说「记到 mem0」，然后 memory_all/history 对账「你说的」vs「抽取器留下的」。

五条事实全文（中文输入被抽成英文，粒度=单条事实；实体/笔记/事实三级粒度对照成型；每轮 add 10–27s，对比 0/1 的毫秒级）：

```
397fc002 ADD User's project recently switched from MongoDB to PostgreSQL around September 2026 due to increased transaction requirements.
1e06df11 ADD User uses Obsidian for note-taking
fd13441a ADD User's favorite editor is Neovim
4f097d34 ADD User's API key is sk-test-12345, they want it remembered for convenient future use
e790cb2c ADD User's project migrated back to MongoDB from PostgreSQL around September 2026 because PostgreSQL operations and maintenance were too troublesome
```

落盘两份，db 即真相、没有人类可直接读改的形态：`qdrant_db/`（单文件 `collection/memlab/storage.sqlite`，`points` 表每行一个 pickle 过的 PointStruct BLOB）+ `history.db`（append-only 事件表）。payload 全貌：

```json
{"user_id": "kal", "run_id": "memlab-run1", "data": "User's project recently switched ...", "text_lemmatized": "...", "hash": "4b2cd...", "created_at": "...", "updated_at": "...", "attributed_to": "user"}
```

表格装不下的洞察：

1. **招牌的自动冲突处理没触发**：R5 判 ADD 不是 UPDATE，history 五条全 ADD。ADD/UPDATE/DELETE 决策本质是 LLM 判断，换模型=换行为。铁律 7（冲突只聚类不裁决）的现实注脚：专门做记忆的系统自己都保证不了裁决一致性。
2. **抽取器幻觉注入原文没有的信息**："around September 2026" 来自抽取 prompt 里的当前时间，被模型织进了事实。抽取产物 ≠ 源文本子集，provenance 必须区分「源文本说的」和「抽取器补的」。
3. 2.x API 陷阱：`search`/`get_all` 必须走 `filters={'user_id':...}`（网上旧示例大量失效）；`from_config` 不配 llm 段会因默认 openai 无 key 崩，纯检索也要给。
4. 待玩：graph memory 需 Neo4j，本次只跑向量+历史；`custom_fact_extraction_prompt` 可换内置抽取器。

自己动手：`cd lab/step2-mem0 && .venv/bin/python feed.py && .venv/bin/python search.py`；重跑全新状态 `rm -rf qdrant_db history.db`；对账 `sqlite3 -column history.db "select substr(memory_id,1,8),event,new_memory from history;"`。

### Step 3：Graphiti（0.30.2）——双时序知识图谱

上轮推荐的是 Zep / Graphiti，实际核实后发现 **Zep Community Edition 已废弃**：getzep/zep 仓库改为 Zep Cloud 的 examples/integrations，CE 代码移入 legacy/ 不再支持（见 ecosystem.md 条目）。双时序图谱的开源本体是 **Graphiti 0.30.2**（MIT，进程内库，活跃），实验载体改为它；Neo4j 5.26 用 docker 起（容器 memlab-neo4j，named volume memlab-neo4j-data）。三件套全指智谱：glm-5.3 抽取 + embedding-3/2048 + reranker 也走智谱（全 lab 默认模型统一为 glm-5.3）。

配环境时撞出的坑（都已解）：

1. **智谱的 json_schema 是软约束**：API 接受该 response_format 但模型无视 schema 形状（要对象给裸数组）。解法 `structured_output_mode='json_object'`，遵从正常。README 警告的「non-conforming LLM」实锤。glm-5.3 上复测结论不变：json_schema 模式返回的是带 ```json 围栏的裸数组、字段名还是它自己编的 `source`/`target`/`relationship`，而 json_object 模式同题 9s 正确返回 2 条边。
2. **openai SDK 3.x 只装 httpx2，graphiti 源码 import 老的 httpx** → pip 手动补装 httpx 才能启动。
3. glm 系思考型模型，小 max_tokens 会被 reasoning_content 吃光（16384 默认值够）。
4. **json_object 模式下偶发不合格 JSON**：模型偶尔给出 `target_entity_name: 5`（int 当字符串）、漏 `relation_type` 这类输出，Pydantic 校验直接抛 ValidationError。graphiti 的 tenacity 重试只覆盖服务端/限流错误（`is_server_or_retry_error`），校验失败不重试，整个 `feed.py` 当场中断。实测 glm-4.6 第一次 smoke 就崩在这、第二次过；正式实验里 glm-5.3 又崩在 R3——这次是把 `EdgeDuplicate` 的负载多包了一层 `{"answer": {...}}`（这次不是冒烟测试，是正式实验真的崩了）。解法 `zhipu_llm.py`：`ZhipuGenericClient` 在 LLM 调用层先校验再返回，单键包装自动拆掉，不合规就重发（temperature=1，重发是真重新采样），3 次都不合规才原样返回、让 graphiti 自己报错。

smoke 已通（smoke.py，2026-09-30 在 glm-5.3 下复跑确认）：2 条 episode → 边写入且 search 首位命中，~48s/episode（每 episode 多次 LLM 调用：实体抽取+去重+边抽取+边去重）。

怎么跑：`feed.py` 逐轮 add_episode（同 5 轮对话，group_id=kal），`search.py` 检索 + Cypher 直读。两个工程措施：输出漂移由 `ZhipuGenericClient` 兜（见坑 4）；feed.py 开跑前先查已入库的 episode 名、跳过已存的轮次，所以崩了直接重跑就是续传，不重复插。

本轮结果：5 episodes / 7 entities / 8 edges，每轮耗时 61 / 25 / 35 / 45 / **559** 秒——最后一轮 9 分钟：边去重候选随图增长，每轮 LLM 调用次数线性涨（中间还有一次空响应触发 tenacity 退避）。落盘=Neo4j 容器，**没有任何人类可读文件**；一条边在库里的全貌：

```json
{"name": "SWITCHED_TO", "fact": "用户的项目最近因事务要求提高而将数据库切换为 PostgreSQL（替换了 MongoDB）",
 "valid_at": "2026-09-30T09:37:18.831237Z", "invalid_at": "2026-09-30T09:42:33.865834Z",
 "expired_at": "2026-09-30T09:51:51.600240Z", "created_at": "2026-09-30T09:38:18.288161Z"}
```

**核心验收点：R5 真的把 R1 标失效了。** R1 的三条边（`user-SWITCHED_FROM->MongoDB`、`user-SWITCHED_TO->PostgreSQL`、`MongoDB-REPLACED_BY->PostgreSQL`）全部拿到 `invalid_at=09:42:33`，正好等于 R5 那条 episode 的 `valid_at`（即我们喂的 reference_time）。四个时间字段各司其职：

| 字段 | 含义 | 这个例子里 |
|---|---|---|
| valid_at | 事实从何时开始成立 | R1 边 09:37:18，R5 边 09:42:33 |
| invalid_at | 事实从何时开始不成立（事件时间，由矛盾的那条 episode 决定） | R1 边 09:42:33 |
| expired_at | 系统何时真的把它标了（系统时间） | R1 边 09:51:51（比 invalid_at 晚 9 分钟，就是 R5 那轮的耗时） |
| created_at | 这条边什么时候写进库 | R1 边 09:38:18 |

valid_at/invalid_at 是「事实时间线」，created_at/expired_at 是「系统时间线」——**双时序指的是这两组，不是四个同义词**。

表格装不下的洞察：

1. **时序质量取决于你喂的 reference_time。** 本次每轮都传 `now()`，所以 valid_at 实际等于喂入时刻，不是从文本推出来的（对比 mem0 直接幻觉注入「September 2026」）。要拿到真实事实时间，就得把原对话的时间戳搬进来当 reference_time——迁移时丢这一列，双时序图谱退化成写入时间图谱。
2. **去重只做失效，不做合并。** R1 的三条同义转述边（「迁移走」/「切换为」/「被替换」）没被并成一条，是三条并存、一起被标失效。更拧的是 R5 的新表述「已迁回 MongoDB」被判成 R1 旧边 `user-USES->PostgreSQL` 的重复：它继承了旧边的 valid_at=09:37:18，又立刻被标 invalid_at=09:42:33——库里出现一条「内容是迁回 Mongo、但这条边自己已失效」的边。铁律 7（冲突只聚类不裁决）在别家是「没做」，在 Graphiti 是「做了，但结论是 LLM 的，会错」。
3. **失效语义不在检索层，在提示词里。** `search()` 默认把已失效的边一并返回（4 个 query 每个都返回 8 条、其中 4 条是死边），靠 prompt 里那句「事实在 valid_at~invalid_at 之间有效」交给下游 LLM 自己判断。迁移只搬 fact 文本、丢掉两个日期字段 = 死事实静默复活，这是铁律 6 的第一个实证样本。
4. **实体抽取没有过滤。** R2 那句「好的，已记录」抽出实体 `assistant`，用户侧抽成 `user`——两个零信息量的节点进了图谱；key 变成一个实体节点 `API 密钥 sk-test-12345`（PII 四连零，且形态从字符串字段升级成图结构：清理要删节点+边+embedding）。
5. fact 抽出来是中文（对比 mem0 抽成英文），graphiti 自带 multilingual 抽取指令；embedding-3 在中文 query（「数据库」）上工作正常。

自己动手：`docker start memlab-neo4j`（OrbStack，镜像已拉）；`cd lab/step3-graphiti && LOG_LEVEL=WARNING .venv/bin/python feed.py`（已入库的轮次会自动跳过，崩了直接重跑续传）；`.venv/bin/python search.py`（4 个 query + 去掉死边的对照 + Cypher 原文 dump）；直接开库 `docker exec memlab-neo4j cypher-shell -u neo4j -p memlabpass "MATCH (a:Entity)-[e:RELATES_TO]->(b) RETURN a.name,e.fact,e.valid_at,e.invalid_at,e.expired_at;"`；清库重来 `docker exec memlab-neo4j cypher-shell -u neo4j -p memlabpass "MATCH (n) DETACH DELETE n;"`。

### Step 4：claude-mem（v13.29.0）——旁路钩子写入

怎么跑：`lab/step4-claude-mem/`，独立 `CLAUDE_CONFIG_DIR` + `CLAUDE_MEM_DATA_DIR`（不碰 `~/.claude`）。装：`npx claude-mem@latest install --no-auto-start`；worker 在 37777；observer 模型走智谱 glm-5.3。

**核心验收点：记忆是钩子写的，不是模型选择写的。** headless 会话里 create/run/edit `hello.py` → 2 条 observation 落 SQLite 并同步 Chroma；而只做一次 Read 的会话产出 **0 条**（被当噪声跳过）。「触发有保证」≠「一定有条目」。

1. 身份是 project（git 仓库名或目录名）：换目录换库，同目录共用。
2. 每次工具调用都过一次 observer 模型——**默认出网、默认烧 token**，这是产品设计不是配置失误。
3. 安装器默认选云端（CMEM Pro）；自托管要显式改 `data/settings.json` 的 provider。

### Step 4b：agentmemory（v0.9.29）——零 LLM 的钩子记忆

怎么跑：`lab/step4-agentmemory/`，独立 `CLAUDE_CONFIG_DIR`；插件装进隔离配置（`claude plugin marketplace add rohitg00/agentmemory` + `claude plugin install`）。服务后台跑，4 个端口（3111/3112/3113/49134），iii-engine v0.11.2。**`agentmemory` 不是全局命令，一律用 `npx -y @agentmemory/agentmemory@latest <子命令>`。**

**核心验收点：不给任何 key 也能记住东西。** REST 往返 remember → smart-search 命中（默认 BM25）；钩子会话 status：Sessions 1 / Observations 3 / Memory 1 / Graph 1 node。

1. observation 和 memory 是两层：observation 直接落，memory 带 `version`/`supersedes`/`strength`，consolidation 默认关。
2. **零 LLM 是默认模式，不是降级模式**——和 mem0 / claude-mem 的假设正好相反。四套里只有它默认不出网。
3. **只隔离了数据，没隔离运行时**：state store 在 `lab/.../data/`，但引擎二进制、pidfile、`engine-state.json`、`preferences.json` 全在全局 `~/.agentmemory/`，端口也是全局独占。再起第二个实例会被守卫拦下（`Starting a second instance here would corrupt the running daemon's REST routing`）——**这句是保护不是故障**；`stop` 读的也是全局 pidfile，停之前先确认停的是哪个实例。

### Step 5：Memoria（v0.5.2）——Git for memory

怎么跑：`lab/step5-memoria/`，Docker 栈（MatrixOne + api:8100，用现成镜像 `matrixorigin/memoria:latest`），CLI 在 `bin/memoria`（未入 PATH）。一键：`bash lab/step5-memoria/smoke.sh`。

**核心验收点：矛盾不裁决，回滚才是答案。** 写入 R1（迁 PostgreSQL）→ 中文 query「数据库」命中（score 0.633）→ 建快照 → 写入矛盾 R5（迁回 MongoDB）→ **两条并存** → 回滚 → R5 消失，只剩 R1。

1. 「记忆治理」= 时间机器（snapshot/branch/rollback/diff），不是更聪明的合并——本仓库长期规划的「撤回闭环」可以直接借这个形状。
2. **embedding 维度锁进 schema**：智谱 `embedding-3` 实测 2048 维，配错就锁死（铁律 5 的现实版）。
3. MCP：`memoria init --tool claude` 生成 `.mcp.json` + 5 条 steering rules；stdio 握手实测 **25 个工具**。

### Step 5b：Panella（0.2.1）——default-deny 的治理箱子

怎么跑：`lab/step5-panella/box`，`uvx panella@0.2.1 up --yes --home "$PWD"`；`init --verify` 五项全 PASS。

**核心验收点：未批准即不可见。** MCP 提交候选 → `{"queued":true,"approval_id":4}` → 检索时**新 nonce 读不到** → operator 用 CLI 批准（`durable_id=1db17d08b8...` 哈希凭证）→ 检索命中。中间那次「读不到」才是产品在工作。

1. **写权限是机制不是约定**：agent 拿不到 approval credential，写入是提案。
2. 这条「未批准即不可见」是可验证断言，可以直接当本仓库 dry-run 闸门（铁律 2）的验收标准。
3. **「已批准」和「已落库」是两步**：实测遇到 `approval N is approved but not yet durable`（finalize 瞬时失败，CLI 让 retry）。迁移管道不能把「审批通过」当成「写入成功」。
4. drawer 的 metadata 里带**检索历史**（谁在何时用什么 query 查过它）——判断可见性别拿整个响应做子串匹配，查询词会自己回显，只能看 hit 的 `content`。
5. `init --verify` 连审批文件的 0600 权限都查——治理凭证的落盘安全也算产品的一部分。

完整环境与隔离表、可复制的体验命令、记录模板、清理方式：见 [step4-5-report.md](step4-5-report.md)。

## 网页端记忆导出形态：ChatGPT / Claude / Gemini

三家能不能把记忆拿出来，答案完全不同。这一节只回答两件事：**格式是什么**、**我们怎么处理**。

证据分三档：**官方文档** = 厂商帮助中心；**第三方核对** = 第三方工具对真实导出包的字段核对；
**本机实测** = 本机确实有/没有的东西。⚠️ **我们手上没有任何一家的真导出包**（本机 `find` 过，
无 `memories.json`、无 Takeout），所以下面的**字段级结论一律是「第三方核对」，不是实测**，
Reader 落地时必须用真导出再核一遍。

| 产品 | 记忆能导出吗 | 载体 | 我们怎么处理 |
| --- | --- | --- | --- |
| ChatGPT | ❌ 导出包**不含记忆** | 无文件，服务端合成层 | 走 [reader-prompts.md](reader-prompts.md) 的 Prompt 抽取；`conversations.json` 只当原始素材 |
| Claude.ai | ✅ 在账号数据导出包里 | `memories.json`（单元素数组） | 直接解析，**新版优先、旧版兼容**（见下） |
| Gemini | ❌ Takeout 只有活动记录 | 无记忆文件，记忆是**派生层** | 走 Prompt 抽取；`MyActivity.json` 当原始素材留档 |

### Claude.ai：`memories.json` 是新旧两代叠在一个文件里

这是三家唯一真正能拿到记忆的。整个文件是**单元素数组**（按账号一条记录），字段如下：

| 字段 | 类型 | 代际 | 含义 |
| --- | --- | --- | --- |
| `memory_files` | array of `{path, content, updated_at}` | **新版** | 文件式记忆。每项一份 markdown，路径形如 `/profile.md`、`/preferences.md`，并为具体人物、主题各开文件 |
| `conversations_memory` | string | 旧版 | 账号级 markdown 文档，**一整块文本**，不是条目数组 |
| `project_memories` | object（项目 UUID → string） | 旧版 | 每个项目一份结构化文本，含 Purpose / Current state / Key learnings / Tools 等小节 |
| `account_uuid` | string | 通用 | 对应 `users.json` 里的账号 UUID |

**处理规则（按顺序）**：

1. **有 `memory_files` 就用它**——它是唯一带 `path` 和 `updated_at` 的字段，也就是唯一自带
   provenance 的字段，直接还原成目录树即可，一条文件一条记录。
2. **没有就退回旧版两个字段**：`conversations_memory` 按小节切分（切不动就整块存成一条，
   迁移报告里标注「已退化」）；`project_memories` 按小节切，或每个项目存成一条。
3. 两代字段**可能同时存在**，这时以 `memory_files` 为主、旧字段作为补充，不要重复计入。

**代际时间线**（官方文档）：2026-07-10 记忆从「每天合成的单一摘要」改成「一组分类条目」；
2026-08-25 起记忆列在 `Settings > Memory > Topics` 下可逐条编辑、跨 chat 与 Cowork 生效、
敏感话题默认不记（要手动开）；**旧版记忆的导出入口只开放到 2026-09-09**，这个窗口已过。

**判断新版旧版**：设置里看到 `设置 > 记忆` 是新版；看到 `设置 > 功能 > 记忆` 是旧版。

**坑**：① 对话量大的账号拿到的是 manifest 指向**多个分批 ZIP**，每个链接只能用一次，
`memories.json` 在**第一批**里；② 设置页一次只显示一份文件，**没有「全部下载」按钮**，
所以导出包是唯一完整快照；③ 导出的 ZIP **不能导入另一个个人 Claude 账号**（Anthropic 明确
不支持个人账号间迁移，导入功能只面向从别家带进来）。

### ChatGPT：导出包里没有记忆，只能问出来

官方对导出的描述只有一句含糊的「聊天记录和其他相关账号数据」；第三方对真实导出包的核对结论是
**自定义指令包含、记忆不包含**。原因是记忆属于「持续更新的合成物」，不是一个可直接落盘的文件。

ChatGPT 的记忆本身是两层叠加：用户显式保存的条目 + 后台定期重写的合成摘要（Memory Summary）。
**官方自己承认这份摘要「不一定包含 ChatGPT 记得的每项细节或来源」**，并建议「想知道它记住了什么
就直接问」——这句话正是我们写 Prompt 的依据。

**处理**：走 Prompt 抽取（[reader-prompts.md](reader-prompts.md)），并且要**跑两遍取并集**，
因为两层记忆一次通常只吐一层。`conversations.json` 留作原始素材，但注意它的形状：
对话是 `mapping[message_id]` 的**树**（要沿 `current_node` 走活跃分支，丢弃被放弃的分支），
时间戳是 Unix 浮点，`content.parts[]` 里混着图片对象和 `null`，都要先清洗。

### Gemini：记忆是派生层，结构上就导不出来

Takeout 里只有两个入口，**都不含记忆**：勾「Gemini」得到 Gems（自定义助手）配置；
勾「我的活动记录 → Gemini 应用」得到对话、生成媒体、上传内容。文档里**没有任何记忆/个性化/
个人上下文相关的导出选项**。

原因和 ChatGPT 不同但结论一样：Gemini 的记忆不是可落盘的条目文件，而是**从活动记录里检索归纳
出来的结果**——「导出活动记录」就是你能拿到的最接近原始素材的东西，**记忆在源数据里，不在结果里**。

**处理**：走 Prompt 抽取；`MyActivity.json` 留档当原始素材。它的字段遵循 Google Data Portability
的 My Activity schema（`header` / `title` / `time` / `subtitles` / `details` / `products` /
`activityControls` / 附件字段等）。

**坑**：① 一条 prompt 一条记录，**没有字段标明属于哪个对话**，重建会话边界只能靠时间戳猜；
② 模型回复常以 HTML 片段存在 JSON 里，要先去标签再 `html.unescape`；③ 最常见的「导出为空」是把
**Gemini** 和 **我的活动记录里的 Gemini 应用**两个勾选框搞混；④ 个人账号的活动记录默认
**18 个月自动删除**，关掉「保留活动记录」后新对话不进历史、也就无从做记忆。

### 对本仓库的三个直接结论

1. **三家只有 Claude 能给文件，另外两家只能给文本**——所以 Reader 要分两类：导出包解析器
   （Claude）和 Prompt 抽取器（ChatGPT / Gemini）。后者见 [reader-prompts.md](reader-prompts.md)。
2. **能拿到的东西和它真正记得的东西不是一回事**：ChatGPT 和 Gemini 的记忆都是服务端合成/派生的
   结果，官方明确承认不完整。这条必须写进迁移报告的 `source_unavailable`，否则就是静默衰减。
3. **provenance 在网页端普遍缺失**：ChatGPT 没有 per-entry 来源，Gemini 连对话边界都没有，
   只有 Claude 的 `memory_files` 带 `path` + `updated_at`。这决定了 Reader 的能力上限——
   报告里 `provenance` 一栏对多数网页端源只能是「无」。

## 体感建立之后的回看清单

- [x] 八套实验系统的**形态总览**（名字对照、三类分法、逐系统机制、五条横向规律）→ 见 [memory-products.md](memory-products.md)
- [x] 网页端（ChatGPT / Claude.ai / Gemini）与 harness（Claude Code / Codex / Gemini CLI / AGENTS.md 家族）的记忆形态调研 → 见 [source-memory-formats.md](source-memory-formats.md) 与 [codex-memory.md](codex-memory.md)，含本机落盘实测与待核实清单
- [x] 三家的**导出格式与处理方式**已定型（ChatGPT 不含记忆 / Claude 新版 `memory_files` 优先兼容旧版 / Gemini 只有活动记录）→ 见上一节；无导出通道的产品走 [reader-prompts.md](reader-prompts.md) 的 Prompt
- [ ] 拿到 ChatGPT / Claude / Gemini 的**真导出**，核对 schema（尤其 Claude 的 `memory_files` 实际命名规律、ChatGPT 是否真不含记忆、Gemini 的 `MyActivity.json` 变体）
- [x] UMP 的 SPEC 定义了什么 → SPEC.md 510 行已读；`*.ump.json` 样例见 `examples/responsibility-confirmed.ump.json`；**导入器只有 `src/importers/filesystem.ts`，不做平台迁移，吞并风险证伪**（2026-10-05）
- [x] AIMEM bundle 语义 → 已从 IETF 原文逐条核实（DNA 五类不可静默衰减、embedding 模型名+维度为**信封级**声明、JCS+SHA-256 checksum、URN 幂等、擦除审计只记发生过）；**草案引用的参考实现仓库 404，无 conformance 语料**（2026-10-05）
- [x] MIF / OKF 的真实文件 → 已克隆 `lab/upstream/MIF`、`lab/upstream/open-knowledge-format`；OKF `bundles/` 有 acme_retail / crypto_bitcoin / ga4 / stackoverflow 四个真样例可直接抄 envelope（2026-10-05）
- [x] Remnic 的 importer 结构 → 8 个源适配器包已定位（chatgpt / claude / gemini / mem0 / supermemory / okf / lossless-claw / weclone）；代码细读待做（2026-10-05）
- [x] 读 OMPI `prior-art/convergence-analysis.md`（与 [memory-products.md](memory-products.md) 对读）与 `working-group/exchange-and-runtime/2026-09-28-common-ground.md` → **完成**；8 条分歧轴 + 最小可行标准六节 + 对读出的 14 项盲区已写进 [ecosystem.md](ecosystem.md) 的 OMPI 条目（2026-10-05）
- [x] 读 MacPaw portable-memory 的 `secretRef` / `tombstone` schema 与 `Conformance/vectors` → **完成**；结论：`secretRef` 是外部 vault 引用（明文密文 NEVER 随包）⇒ **PII 闸门必须在 Reader 出口**；`tombstone` 是物理删内容 + 永久留墓碑 + proof-of-reach（2026-10-05）
- [x] 读 Memanto `memanto/cli/migrate/mappers.py` 的六个 mapper，标出字段损失点 → **完成**；逐 mapper 丢弃清单 + `map_zep` bi-temporal 处理 + 写盘路径二次损失（`expires_at`/`ttl_seconds` 静默丢）+ 三条与铁律正面冲突的行为（默认非 dry-run / dry-run 仍调远程模型 / migrate 流程无 PII 闸门）已写进 [ecosystem.md](ecosystem.md)（2026-10-05）
- [x] Remnic importers + contradiction-review + belief-ledger → **完成**；ChatGPT/Claude/Gemini 真实 schema（含新旧版差异）+ 10 条静默衰减清单 + 两套冲突机制（含 pairId 与 `entityRef::attrName` 复合键）已写进 [ecosystem.md](ecosystem.md)（2026-10-05）
- [x] UMP SPEC + `src/` → **完成**；记录字段表、L0–L3、§5.3 再水化实现细节（`requireValidSignature` 是死代码）、§2.9 边界声明、§6.3 Markdown 必须用 JSON front-matter（规范样例的 YAML 会让官方解析器报错）已写进 [ecosystem.md](ecosystem.md)（2026-10-05）
- [ ] 从 OMPI `prior-art/` 挖出的未跟踪项目名里，挑「值得单查」的做第二轮：**PTC**（溯源+信任凭证形状）、**W3C AI Agent Memory Interoperability CG**（唯一专门的记忆互操作工作组）、**Letta Trajectory Library**（四级身份派生）、**EngramSpec/8mem**（CORRECTIONS/EVOLUTION 四对象）、**Context Nest**（forget 协议 + 哈希链版本）、**MINJA**（记忆注入攻击）
- [ ] 读 ai-memory `docs/research-2026-landscape.md` 与 `comparison.md`，与我们的生态判断对读，摘取漏掉的项目名
