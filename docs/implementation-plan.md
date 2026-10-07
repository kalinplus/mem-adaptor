# 初版实施方案：MVP D1

**文档定位：初版实施方案，供历史参考。** 本文保留实施路径与验收设想，
不再追加进展记录，也不代表当前实现或人工验收状态。
当前任务、验证证据与交接以 GitHub Issue/PR 为准；实际目录、依赖版本与接口签名以源码为准。

这份文档把 [design.md](design.md) 的设计落到「先写什么、怎么验收」。设计本身以 design.md 为准，
这里只增加**实现层的选择**（语言、仓库布局、接口签名、里程碑）。两边冲突时以 design.md 为准，
改设计先改 design.md。

范围：详写 D1，D2–3 只列入口和要在那时拍板的点。节奏按 walking skeleton（先用桩把全链路跑通，
再逐个垂直切片换成真实实现）：M2 结束时 `plan → apply → 回执` 就能端到端跑通，之后每个里程碑只替换一段。

## 0. 已定的实现层选择

| # | 选择 | 理由 | 拍板 |
|---|---|---|---|
| IMP-1 | **核心用 Rust**（引擎、Reader、Writer、CLI），**conformance 执行器用 Python** | D1 是纯本地数据处理，没有机器学习依赖；类型系统能把铁律变成编译期约束（见 §2）；单文件分发对普通用户友好。两种语言让执行器在结构上不可能 import 实现（铁律 8、DEC-7） | 用户，2026-10-05 |
| IMP-2 | **手写 JSON Schema 为正本**，Rust 类型另写，测试保证两边一致 | 报告 schema 是核心资产，要语言中立，conformance 套件直接读它 | 用户 |
| IMP-3 | **会话记录（ChatGPT / Claude 的 `conversations.json`）D1 只登记进源清单并计数，不转成记忆** | 不调模型，本地抽不出像样的记忆；原样转成记录会淹没家目录。以后接 opt-in 抽取（DEC-12） | 用户 |
| IMP-4 | **Markdown 一个文件一条记录，不按标题切分** | 切分就是改写原文；OKF 家也是一文件一条 | 用户 |
| IMP-5 | Reader/Writer 每个一个 crate，核心 crate **不依赖**任何适配器 crate，由 CLI crate 注册 | 「引擎不 import 具体适配器」由 Cargo 依赖图强制，而不是靠约定 | 本计划 |
| IMP-6 | 跨语言插件协议（子进程 + JSON）不在 D1 做；D1 的适配器全部在仓库内 | Rust 没有稳定 ABI，社区插件要么编进主程序、要么走子进程协议。等有第一个外部贡献者再定 | 本计划 |

## 1. 仓库布局

```
mem-adaptor/
  Cargo.toml                       # workspace
  schema/                          # 正本，语言中立（IMP-2）
    canonical-record.schema.json
    plan-report.schema.json
    receipt-report.schema.json
    approval-receipt.schema.json
    config.schema.json             # config.toml 解析后的结构
    vectors/                       # 测试向量：valid/ 与 invalid/，每个附期望结果
  crates/
    core/                          # mem-adaptor-core：canonical 类型、Reader/Writer trait、
                                   #   注册表、九阶段引擎、闸门、去重、报告、计划摘要
    reader-markdown/               # 本地 Markdown / Obsidian / Claude Code memory / OKF 家
    reader-chatgpt/
    reader-claude/
    writer-ump/
    writer-okf/
    cli/                           # 二进制 mem-adaptor：注册适配器、配置、交互
  conformance/                     # Python（uv 项目），只读 schema/ 与产物，不 import 任何 Rust
  fixtures/                        # 合成的源数据（目录形式；ZIP 在测试里现打）
  docs/  lab/                      # 现有，不动
```

依赖方向：`cli → {reader-*, writer-*} → core`。`core` 不依赖任何适配器；适配器之间互不依赖。

## 2. 核心接口（Rust 签名草图）

签名是契约，名字和字段在 M1–M2 实现时可以微调，但下面四条约束不改：

```rust
pub trait Reader {
    fn id(&self) -> &'static str;                         // "chatgpt-export"
    fn version(&self) -> &'static str;
    /// 第 1 阶段：看文件清单和内容，声明认领哪些文件、各属哪一层
    fn claim(&self, inv: &FileInventory) -> Vec<Claim>;
    /// 第 2–3 阶段：解析 + 归一
    fn read(&self, claim: &Claim, src: &SourceFs) -> Result<ReaderOutput>;
}

pub struct ReaderOutput {
    pub source_records: Vec<SourceRecord>,
    pub records: Vec<CanonicalRecord>,
    pub anomalies: Vec<Anomaly>,
    pub registered_count: u64,
    pub deleted_count: u64,
    pub source_unavailable: Vec<SourceUnavailable>,
}

pub struct SourceRecord {
    pub canonical_id: String,                  // Per-record association across source systems
    pub source_record_id: String,
    pub source_locator: String,
    pub fields: serde_json::Value,             // Raw source fields
    pub field_map: Vec<FieldMapping>,          // Per-record mappings with optional rule
    pub unmapped: Vec<UnmappedField>,
}

pub trait Writer {
    fn id(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;     // 能承载哪些 canonical 字段、能否回读
    /// 第 6 阶段：纯函数，只预测，不碰目标
    fn plan(&self, rec: &CanonicalRecord, prev: Option<&ReceiptEntry>) -> Planned;
    /// 第 8 阶段：只有拿到 WriteToken 才能调
    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<Vec<Written>>;
    /// 第 9 阶段：用目标自己的读法，还原成可比对的形状
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>>;
}
```

四条用类型保证的约束：

1. **`WriteToken` 只能由引擎构造**（构造函数在 `core` 内私有）。引擎在审批凭证的计划摘要与重算结果一致后才发 token。
   Writer 没有 token 就调不了 `write`：DEC-11 的 dry-run 截断和 DEC-3 的摘要核对是编译期保证，不是文档约定。
2. **处置是穷举的 enum**：`Accepted | Transformed{changes} | Omitted{reason} | Unresolved{reason} | Rejected{rule}`，
   原因也是 enum（`DuplicateOf`、`AlreadyMigrated`、`DeletedInTarget`、`TargetUnsupported{field}` …）。
   新增一种处置时，所有没处理它的地方都会编译失败。验证结果同理：`Verified | Mismatch{diff} | Unverifiable{why}`。
3. **闸门命中只有位置、没有值**：`Finding { rule_id, tier, field_path, byte_span, disposition }`。
   检测函数只返回 span，命中值从不离开 `content`，所以任何序列化路径（报告、日志、stdout）都拿不到它（DEC-1）。
4. **`accepted` 的判定可机器复核**：`content` 字节不变、且回读能还原出全部声明承载的 canonical 字段，才算 `accepted`；
   其余承载情况一律 `transformed` 并列出改写。id 桥接（DEC-16）不算改写。

**计划摘要**（DEC-3）= `sha256(JCS({records, targets, writers, gate_policy}))`，其中 JCS 是 RFC 8785
JSON 规范化（JSON Canonicalization Scheme），`records` 是按 `canonical_id` 排序的
`{canonical_id, content_hash, record_hash, 处置预测}`。
`record_hash = sha256(JCS(canonical record))`，绑定正文、scope、consent、来源版本等元数据；
逐目标预测的 `prior_write` 同时绑定覆盖前的历史目标状态。
**摘要的全部输入都写在计划报告里**，所以 conformance 执行器
不用 import 引擎就能独立重算。时间戳、运行 id 不进摘要，否则同一计划重跑摘要会变。

**`canonical_id`** = `sha256(source.system ‖ 0x00 ‖ satellite_id ‖ 0x00 ‖ source_record_id)` 取前 20 字节，
转无 padding 的小写 base32，共 32 字符（卫星维度按 Issue #22 增加，见 design.md DEC-20）。
直迁模式 `satellite_id` 为空串、仍参与哈希。确定性派生，同一条源记录重跑得到同一个 id；家模式下 id 只在首次
从卫星读入时计算，从家读回时按 `mem_adaptor:` 扩展块恢复、不重算（DEC-20），OKF 家的文件名（DEC-19）跨次稳定。
源里没有 id 的记录，`source_record_id` 按 Reader 规定取：Markdown 用相对路径，Prompt 抽取文本用归一化后的行内容哈希
（这一类内容一改就成了新记录，计划报告里会显示为一增一删，见 §4 M5）。

## 3. 选定的依赖

以下为方案中的依赖选择，不作为当前依赖清单；需验证的行为在对应里程碑里先做小实验再定。

| 用途 | crate | 备注 |
|---|---|---|
| CLI | `clap` 4 | |
| 日志 | `tracing` + `tracing-subscriber` | 级别由 `LOG_LEVEL` 控制；阶段前缀 `[S1]`…`[S9]` |
| JSON / 规范化 | `serde_json`（`preserve_order`）、`serde_jcs` 0.2 | 需验证 UTF-16 键排序与数字规范化 |
| JSON Schema 校验 | `jsonschema` | 只在测试和 `core` 的第 3 阶段校验里用 |
| YAML frontmatter | `serde-saphyr` 1.3.0 | 需验证配合 `serde_json/preserve_order` 保留未知键、嵌套类型与键顺序（OKF §4.1） |
| ZIP | `zip` 8.6.0 | 选择稳定版，仅启用 deflate；不使用预发布版 |
| TOML 配置 | `toml` | |
| 哈希 / 编码 | `sha2`、`data-encoding`（base32） | UMP 的 blake3 只在 L3 需要，D1 不引入 |
| 正则 | `regex` | 需验证 Gitleaks（MIT）规则兼容性；复用子集并保留署名，补本地密码赋值规则 |
| 快照测试 | `serde_json` 值比较 | 使用合成 JSON golden 文件；固定运行字段/路径后重算摘要，不额外引入 insta |
| 交互 | `dialoguer` 或直接读 stdin；TTY 判定用 `std::io::IsTerminal` | |

## 4. 里程碑

每个里程碑都要做到：`cargo test` 与 `cargo clippy -- -D warnings` 通过，验收项逐条勾掉，
文档有变化的同步改（README 入口、design.md 中被实现确认或推翻的事实）。

### M0 准备

- 开始实施时**不是 git 仓库**，先 `git init`。`.gitignore` 已挡住密钥和实验数据，还要补两处：
  加 `target/`（Rust 构建产物）；现有的全局 `*.jsonl` 规则会把合成 fixtures 也挡掉，要加例外 `!fixtures/**/*.jsonl`。
  根目录的 `.mcp.json` 进库前先看一眼有没有凭据。
- 建 workspace 与空 crate，CLI 能 `mem-adaptor --version`。

验收：`cargo build` 通过；`git status` 里没有 `.env`、`lab/**/data`、`lab/upstream/`、`target/` 这类不该进库的东西。

### M1 schema 正本 v0

- 手写五份 JSON Schema（draft 2020-12），字段对齐 design.md §2（canonical model）和 §3（报告顶层字段、五态、验证结果）。
  D1 用不到的字段（向量层、冲突裁决）先写进 schema，标为可选，避免 D2 再改 schema 版本。
- 报告顶层带 `schema_version`；计划报告含：源清单（含无人认领的文件和只登记不转换的会话记录及其计数）、
  逐条预测、目标能力声明、模型调用事实（D1 恒为空列表，但字段必须在）、闸门策略及来源、计划摘要及其全部输入、bundle manifest。
- 写 `vectors/valid` 与 `vectors/invalid`，每个 invalid 向量注明它违反哪条约束。
- Rust 类型与 schema 一致性的测试：① 所有 valid 向量能反序列化成 Rust 类型、再序列化后仍通过 schema；
  ② 每个 Rust 构造出的示例通过 schema；③ 所有 invalid 向量被 schema 拒绝。

验收：三类一致性测试通过；随手删一个必需字段，测试会失败。

### M2 walking skeleton：端到端跑通

用最薄的真实实现把九个阶段串起来：

- 引擎九阶段全部存在，其中闸门（无规则）、去重（不去重）、回执链（不读旧回执）先是直通桩。
- `reader-markdown` 最小版：一个 `.md` 文件一条记录，只读正文。
- `writer-okf` 最小版：按 DEC-19 目录约定写 `memories/<canonical_id>.md`，回读逐字节比对。
- CLI 两步：`mem-adaptor plan <源> --to okf:<目录>` 只产出计划报告；
  `mem-adaptor apply <计划报告>` 命令行确认 → 生成本地审批凭证（`approval-receipt`，绑定计划摘要）→ 重算计划 → 摘要一致才拿到 `WriteToken` → 写入 → 回读 → 回执报告。
- 同时做 YAML crate 的小实验（§3）。

验收：

- `plan` 之后目标目录**一个字节都没变**（测试比对目录哈希）。
- `apply` 后回执报告通过 schema，所有记录 `verified`。
- `plan` 与 `apply` 之间改一个源文件，`apply` 拒写，并说明摘要不一致。
- 日志按 `[S1]`…`[S9]` 打出每阶段输入/输出规模。

### M3 引擎切片

逐个把桩换成真实实现，每段单独提交：

1. **第 1 阶段解压与源识别**：ZIP 解到临时目录，拒绝跳出目录的路径（zip-slip）、拒绝符号链接；
   每个文件要么被某个 Reader 认领，要么进「无人认领」清单（DEC-10）。
2. **第 3 阶段引擎校验**（DEC-17）：canonical 记录过 schema；源记录的每个字段路径要么在 `field_map`、要么在 `unmapped`，
   两边都没有的由引擎标出并进计划报告。
3. **第 4 阶段密钥闸门**（DEC-1）：
   - 规则：先验证 gitleaks 规则能否直接被 `regex` 编译，能就取一个子集当数据文件随仓库分发（保留 MIT 署名），不能就手写常见几类（OpenAI/Anthropic/GitHub/AWS key、私钥块、`password=` 赋值）。
   - 策略 `pass`/`block`；`block` 下命中 → `rejected`；规则级白名单记入报告。高危 PII 的策略字段 D1 就留好，检测到 D2–3 才接。
   - 每次运行 stdout 打一行摘要；非交互且无配置时标「策略来自默认值，未经用户选择」。
   - `secretRef` 类源条目替换为引用，与策略无关。
4. **第 5 阶段精确去重 + 回执链**（DEC-6、DEC-18）：语义 metadata 一致且正文哈希或空白归一哈希相同 → 后者 `omitted duplicate_of`；
   读入上一次回执，实现 DEC-18 表里的五种情况；没有旧回执且目标非空时，计划报告给出防复活失效的显式警告。
5. **第 6 阶段计划报告**补齐 M1 定义的全部顶层字段。

验收（除各切片的单元测试外，以下几条用集成测试固定下来）：

- 夹带 `../` 和绝对路径的恶意 ZIP 被拒，临时目录外无文件产生。
- fixtures 里埋若干假密钥；跑完后在计划报告、回执报告、stdout、stderr、日志文件里**搜不到任何一个命中值**。
- 同一输入连跑两次：第二次计划里全部 `already_migrated`，`apply` 不写任何文件。
- 在目标里手删一条已 `verified` 的记录后重跑：该条 `deleted_in_target`，不被写回。
- Reader 故意漏报一个字段：引擎标出，计划报告里可见。

### M4 三个 Reader

源数据形状以 [source-memory-formats.md](source-memory-formats.md) 为准。
初版按文档合成 fixtures，不以真实导出包作为开工前提
（形状参照 Remnic 的 MIT 合成 fixtures，但自己写），所以这些 Reader 产出的记录 `evidence_level` 为「第三方核对」，直到 M7 用真导出核过。
这句话限定网页导出；本地 Markdown 按实际读取标记实测，类别未知仍标推断，OKF 恢复原证据等级。
未知源 metadata 保存在可选 `source_extra`，绑定完整记录摘要并接受闸门检测，
报告只列路径和保留位置；每条源记录独立携带映射与未承载清单。

**reader-markdown**（含 OKF 家、Obsidian、Claude Code 的 `memory/` 目录）

- 一文件一条（IMP-4）；frontmatter 已知键进 `field_map`，未知键进 `unmapped`，原样保留以便写回。
- 识别 OKF 家：读回 `mem_adaptor:` 扩展块，恢复 `canonical_id` 与其余字段，保证家能作为源参与去重（DEC-19）。
- Claude Code：`MEMORY.md` 是索引，登记但不当记录；topic 文件一条一记录，`metadata.type` 进 `source_kind`，`originSessionId` 进溯源。
- 空目录产出「源存在但无数据」，不报错（source-memory-formats.md 的不变式用例）。

**reader-chatgpt**

- saved memories JSON（若导出里有）：兼容 `{memory:[...]}` 与顶层数组两种形状，`deleted` 条目跳过并计数。
- Prompt 抽取文本（`*.chatgpt.md`，[reader-prompts.md](reader-prompts.md) 的 `[日期或 unknown] [类别] 内容`）：
  坏行进异常清单，不中断；只有日期的值保留为自报 metadata，不补造午夜时刻。
- `conversations.json`：只登记、计数（IMP-3）。
- `user.json` 里的自定义指令：字段位置未经真导出核实，D1 先登记不解析。

**reader-claude**

- `memories.json`：有 `memory_files` 就用它（一文件一条）；没有就退回旧版，`conversations_memory` 整块一条、
  `project_memories` 每个项目一条（不切分，IMP-4）。
- `projects.json`：`docs[]` 每份一条，非空 `prompt_template` 一条。
- `conversations.json`：只登记、计数。

**`dna_class` 的赋值**（DEC-4）：Reader 按显式映射表给，映射表进报告。初版：`profile`→`identity`、`preference`→`preference`、
`instruction`→`procedure` 为 DNA 类；`project`、`tool` 及无法判断的为 standard，且无法判断时 `evidence_level` 标「推断」。
这张表要你过目（见 §6）。

验收：每个 Reader 对自己的 fixtures 产出的计划报告有稳定、脱敏的 JSON 快照；源字段覆盖校验零遗漏；
ChatGPT / Claude 的 ZIP fixtures 的源清单里，会话记录被认领并计数、未被转成记录。

### M5 两个 Writer

**writer-okf**（默认的家，DEC-19）：补齐 DEC-19 的字段落位表；`index.md` 按 scope 重新生成、`log.md` 追加一条；
不写 `verified`；向量不进家，只在扩展块记模型名和维度。回读：重新解析写出的文件，按 §2 第 4 条判定 `accepted`/`transformed`。

**writer-ump**（DEC-8）：写 `*.ump.json` 数组；`id` 用 `urn:ump:` + `canonical_id`（已是小写 base32，桥接可逆）；
`owner` 用稳定 opaque 字符串；`provenance.actor_kind="import"`；embedding、丢失字段清单、源 kind 进 `body.structured`；
`kind` 按映射表转成 UMP 的五种 kind，源侧原值 `source_kind` 保留在 `body.structured`；
回读能还原 `source_kind`，所以按 §2 第 4 条仍是 `accepted`，映射关系写进字段映射表。
D1 不写 `*.ump.md`（需要时再加，届时 front-matter 必须是 JSON）。
校验：用 vendored 的 UMP 官方 JSON Schema（`lab/upstream/universal-memory-protocol/src/schema/ump-record.schema.json`，Apache-2.0）校验每条输出。

官方 schema、license 和来源需通过 `crates/writer-ump/schema/` 随包分发，运行不依赖 `lab/upstream`。
原生 `target_hash`、共享 `artifacts` 与 `target_map` 需进入审批依据；
WriteToken 核对 Writer 读取的原生字节。源创建时刻缺失时明确标为目标迁移时间，更新保持不变；
非空 `consent.redact` 未经处理时拒写。具体映射和限制见 [M5 方案](m5-writer-proposal.md)。

两个 Writer 都在 `capabilities()` 里如实声明承载什么、不承载什么，计划报告据此预测。

验收：两个 Writer 对同一组 canonical 记录的产物有快照；UMP 输出全部通过官方 schema；
OKF 家能被 reader-markdown 读回，且读回的 canonical 记录与写入前一致（家的往返测试）。

### M6 CLI 与配置

- `mem-adaptor init <家目录>`：建 DEC-19 目录骨架，询问闸门策略（默认放行，并说明后果），写 `.mem-adaptor/config.toml`；
  提示「家目录推到公开远端等于公开全部记忆」。不替用户 `git init`，只提示。
  卫星登记表不在 init 写入，只在 apply 批准后追加进 config（design.md DEC-20）。
- `mem-adaptor plan <源> --home <家目录>`：家模式下目标、策略、旧回执都从家目录取；回执存到
  `.mem-adaptor/receipts/<卫星 ID>/<运行 id>.json`（DEC-19），计划报告存到同级的 `.mem-adaptor/plans/<卫星 ID>/<运行 id>.json`
  （DEC-19 的目录约定里没有列计划报告，这是本计划补的）。旧回执归属按卫星 ID 核对，
  替代 `crates/core/src/engine.rs:266-269` 的绝对路径比较；直迁保留 location 比较（DEC-20）。
- 卫星解析顺序、短编号派生、登记表写入时机与搬家检测按 design.md DEC-20；家目录编辑策略与四种变化规则按
  DEC-21（含 OKF Reader/Writer 归属判断修复：归属按扩展块存量证据，不再按值相等猜测）。
- 直迁：首次运行询问策略，写入用户级配置 `~/.config/mem-adaptor/config.toml`。
- `apply` 里集中处理人要决定的事（D1 实际只有 DNA 类 `unresolved`）；非交互运行时这些条目保持不写，并在回执里列出。
- 退出码区分：成功、有 `rejected`/`unresolved`、拒写（摘要不一致）、错误。
- `schema/config.schema.json` 变更方案（随 M6 实施；canonical-record / plan-report / receipt-report 与 vectors
  的变更见 design.md DEC-21 后果 E）：顶层新增可选 `satellites` 数组，每项为
  `{id（pattern "^[a-z2-7]{8}$"）, label, path（可选）, system, created_at（date-time）}`、
  `additionalProperties: false`；顶层 `required` 不变（直迁的用户级配置没有 `satellites`）。

验收：用 tuistory（终端交互测试工具）跑一遍 `init` 与交互式 `apply`；非交互模式（stdin 非 TTY）下不卡住、按默认放行并标注来源。

### M7 conformance 执行器与真实数据验收

**conformance（Python，`conformance/`）**：只读 `schema/`、`vectors/` 和一次运行的产物目录，不 import 任何 Rust 代码。检查：

- 计划报告、回执报告、审批凭证通过 schema；
- 用报告里记下的输入**独立重算计划摘要**，与报告声明的一致，与审批凭证绑定的一致；
- 回执的身份映射完整（每条写入的记录都有源 id ↔ canonical id ↔ 目标 id）；
- 每条非 `accepted` 的处置都带原因或改写内容；
- 向量里埋的已知假密钥不出现在任何产物里。

这一步超出 AGENTS.md 对 D1 的列表，但它是验证「报告 schema 是核心资产」这句话的唯一独立手段，所以放在 D1 收尾。
写入后能否被目标检索找回的探针（§6 Q5）不在这里做。

**真实数据验收**（只读源、输出到临时目录、**不进库**）：

- 本机 Claude Code 的 `~/.claude/projects/*/memory/` → 临时 OKF 家，通读计划报告。
- 你若能提供真 ChatGPT / Claude 导出包，核对 [source-memory-formats.md](source-memory-formats.md) 待核实清单 #1、#2、#4，
  核实后把对应 Reader 的 `evidence_level` 提为「实测」，并回写那份文档。

D1 完成的标准：M0–M7 验收项全部通过；用真实的 Claude Code 记忆跑一次完整的「卫星 → 家」，
计划报告人能读懂、回执全部 `verified`、conformance 执行器全绿。

## 5. 横切要求

- **日志也是出站路径**：日志只打规模、id、规则 id、耗时，不打记录正文，更不打命中值（M3 的测试需覆盖日志）。
- **报告是敏感文件**：默认写到家目录或用户指定目录，不写到仓库内；文档和 CLI 首次运行时提示。
- **fail fast**：解析错误在系统边界（源文件、配置、用户输入）报清楚是哪个文件哪一行；内部不写防御代码。
  单条源记录坏了进异常清单继续跑，整个文件不可解析就报错退出。
- **测试数据全部合成**：fixtures 里不放任何真实记忆；假密钥用明显的测试前缀。

## 6. 实现中要你过目的点

不阻塞开工，到对应里程碑时提出来：

| 时机 | 要你看的 | 我的默认 |
|---|---|---|
| M1 | schema v0 的字段与命名（核心资产，定了以后改就是 schema 升版） | 对齐 design.md §2、§3 |
| M3 | 密钥规则的来源：复用 gitleaks 子集还是手写 | 能编译就复用 |
| M4 | `dna_class` 映射表；逐条未知 metadata 的 `source_extra` 通道 | 按上表分类，通过内部可选字段保留未知 metadata |
| M5 | UMP `kind`；OKF title；缺失创建时刻；完整目标哈希与共享产物审批绑定 | 保守 kind、首行 title、明确迁移时间，完整目标保护 |

## 7. D2–3 入口（届时再细化）

| 项 | 要在那时拍板的点 |
|---|---|
| Mem0 Writer（HTTP，`infer=False`） | 能力声明要写明 `created_at` 变成迁移时间、`role=system` 被跳过、目标侧会调 embedding（DEC-15、DEC-12）；§6 Q9 远程去向是否单独一档闸门 |
| basic-memory Writer | 文件式，复用 writer-okf 的大部分逻辑 |
| PII 检测（接现成 NER） | Rust 侧没有成熟的 NER：在「Presidio 作为本地子进程」与「ONNX 模型进程内推理」之间选；中文 PII 的覆盖要实测 |
| 审批钩子（Panella 式哈希链凭证） | 把 D1 的本地审批凭证换成可插拔后端，在 M2 预留所需接口 |
| 删除语义映射（§6 Q6） | 需先完成设计 |
