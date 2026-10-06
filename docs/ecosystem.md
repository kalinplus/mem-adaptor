# 生态参考：外部项目地址

调研核实过的外部项目清单。地址均经检索核实可公开访问。

本地参考仓克隆在 `lab/upstream/<name>/`（`--depth 1`，2026-10-05 拉取），
含 Memanto、OMPI、MacPaw portable-memory、UMP、Remnic、MIF、OKF、
memcommons、MemOS、ai-memory、mcp-memory-service、Supermemory，
以及撞名的 `open-memory-protocol-aiakashic`。

## 调研优先级分档

2026-10-05 基于材料 + 检索的判定。分档只回答「值不值得投时间」，不代表地址可信度。

| 档 | 项目 | 一句话理由 |
|---|---|---|
| 🔴 立刻 | **Memanto** | 同题竞品，`memanto migrate` + OKF 导出/无损导入已商品化 |
| 🔴 立刻 | **OMPI**（Open Memory Protocol Initiative） | 活规范家族，定义了 Markdown 记忆目录 + 核心/延迟加载契约 |
| 🔴 立刻 | **MacPaw portable-memory** | 公司背书 + 论文 + PyPI/Swift 双实现 + 版本化 RFC 流程 |
| 🟠 本轮 | UMP SPEC | MVP 默认导出目标，需核实其导入器边界 |
| 🟠 本轮 | AIMEM bundle | canonical model 的语义上限由它决定 |
| 🟠 本轮 | Remnic importers 代码 | 离我们 M1 最近的实现，8 个源适配器 |
| 🟠 本轮 | Supermemory 的 mem0→迁移文档 | 唯一把迁移写成工程流程的商业玩家 |
| 🟠 本轮 | Claude / Gemini 原生导入 | 零成本 Writer 目标，格式细节要抄准 |
| 🟡 观察 | MIF、MemOS、ai-memory、mcp-memory-service、memcommons | 有信息但已提炼完定位策略或不构成格式 |
| ⚪ 不调研 | ai-akashic OMP、UHP、MemoryPlugin/MemoryLake/MemX、MemU/cognee/Hindsight/LangMem/OpenClaw 插件群 | 撞名、正交、营销内容、或记忆引擎而非迁移工具 |
| ✅ 已提炼 | A2M、Panella、Memoria、Zep/Graphiti、PAM、mem0、basic-memory | 模式已进铁律或已完成实验 |

**执行状态**：🔴 与 🟠 两档已于 2026-10-05 完成实测调研，结论写在各项目条目的
「实测发现」小节（UMP / OMPI / portable-memory / Memanto / Remnic 五处）。
本轮新挖出的未跟踪项目名单独成节：见下方「未跟踪项目名（待分档）」。
🟡 与 ⚪ 两档维持原判。

### 三条改判断的结论

1. **OKF 已是事实汇合点。** Memanto 用它、MIF 做它的超集、Remnic 有 `import-okf`、
   Memanto bounty 的社区适配器全选它当可移植落点。反向验证了
   「Writer 只做 UMP JSON 和 Markdown/OKF」的决定。但 OKF 是「谁都能填的空信封」——
   Memanto 已往 frontmatter 里塞 `x_memanto` 私有块。我们的 Markdown Writer
   必须显式处理未知字段，这是铁律 6 的落地点。
2. **网页端 Writer 的形态可能不是文件，是一段文本。** Claude 和 Gemini 的记忆导入
   都是粘贴文本框，Anthropic 官方 Prompt 要求 `[date] - content`，与
   [reader-prompts.md](reader-prompts.md) 的 `[日期或未知] [类别] 内容` 只差类别字段。
   Gemini 额外收 `.zip` 聊天记录，可直接吃 ChatGPT / Claude 的官方导出。
3. **「能不能迁」不再是差异化。** Memanto 证明迁移动词（migrate / dry-run / export /
   import）已商品化。护城河只剩 PII 出站闸门、审批钩子不信任自声明 owner、
   带 schema 的机器可验证报告、conformance 不 import 实现、冲突只聚类不裁决——
   即铁律 2/3/4/7/8。

## 实现参考：密钥检测规则

### Gitleaks — 密钥泄露检测工具

- Repo: https://github.com/gitleaks/gitleaks
- Rules: https://github.com/gitleaks/gitleaks/blob/master/config/gitleaks.toml
- MIT，Copyright (c) 2019 Zachary Rice。
- 2026-10-05 检索核实并浅克隆到 `lab/upstream/gitleaks/`；
  参考提交 `b58d3f102cf3a2c84cb7f923d05c25c9b1aed84b`。
- 六条候选签名正则已通过 Rust `regex` 实际编译实验；规则选择与语义边界见
  [M3 契约补充](m3-contract-proposal.md)。只作为规则数据参考，不运行其代码，
  不把本地克隆作为产品或测试依赖。用户已确认复用，数据与 MIT 署名在 `crates/core/rules/`。

## 映射目标（Writer 侧）

### UMP — Universal Memory Protocol

- Repo: https://github.com/edihasaj/universal-memory-protocol
- Docs: https://universalmemoryprotocol.io/
- SPEC: https://github.com/edihasaj/universal-memory-protocol/blob/main/SPEC.md

v1.0 stable，SPEC.md 510 行。六操作（recall / remember / get / revise / forget /
feedback），记录形状 kind / body / scope / provenance / consent，DID 签名，
L0–L3 一致性分级，MCP / HTTP / file 三种绑定。审计日志服务器本地、不随记录走。
注意：前身是 Agent Memory Protocol (AMP) v0.1，后改名 UMP——格式层名字通胀的实例。
本仓库的默认导出目标之一。

SPEC 关键节：§2.3 bi-temporal time、§2.9 admission and responsibility、
§5.3 injection-resistant rehydration（MANDATORY，与 PAM 同思路）、
§6.3 Markdown projection (`*.ump.md`)、§7 conformance。

**吞并风险已证伪**：`src/importers/` 只有一个 `filesystem.ts`，它导的是自家
`*.ump.md`，不是平台迁移导入器。UMP 不做 ChatGPT/Claude 抽取，与我们不重叠。

#### 实测发现（2026-10-05，读 SPEC.md + src/）

**先更正一条记录**：核心是 `capabilities` / `recall` / `remember` / `get` / `revise` /
`forget`；`feedback` 与 `subscribe` 是 §3.8 的**可选 Full-tier ops**，不在核心六操作里。
版本 `1.0` stable，`CHANGELOG` 有 1.0.1（2026-07-30，只加澄清），但记录仍 emit `"1.0"`。

**记录字段**（三处一致：SPEC §2、`src/types.ts:118-132`、`src/schema/ump-record.schema.json`）：

| 字段 | 必需性 | 约束 |
|---|---|---|
| `ump` | 必需 | 字面量 `"1.0"` |
| `id` | 必需 | 正则 `^urn:ump:[a-z2-7]+$`（**小写 base32**，不含 0/1/8/9） |
| `kind` | 必需 | `semantic` / `episodic` / `procedural` / `working` / `identity` |
| `body` | 必需 | 必含 `text: string`；可选 `structured?: object`（**唯一自由逃生口**） |
| `scope` | 必需 | 必含 `owner`（string）+ `visibility`（`private`/`shared`/`public`）；可选 `user`/`project`/`agent`/`session` |
| `time` | 必需 | 必含 `created`（date-time）；可选 `observed`/`valid_from`/`valid_to` |
| `lifecycle` | 可选 | `confidence`/`salience` ∈ [0,1]；`decay`；`status` ∈ `active`/`candidate`/`tombstoned` |
| `supersedes` / `superseded_by` | 可选 | string[] |
| `relations` | 可选 | 保留词 `about`/`contradicts`/`depends_on`/`derived_from`/`duplicate_of` |
| `provenance` | **L2+ 必需** | 必含 `actor`/`actor_kind`/`method`；`actor_kind` ∈ `user`/`agent`/`model`/`import`/`scan` |
| `consent` | 可选 | `retention`（ISO-8601 duration，须 `P` 开头）/`exportable`/`redact`（JSON-path 数组） |
| `integrity` | L2 可选 / **L3 必需** | `content_hash`（`^blake3:`）/`signature`（`^ed25519:`）/`signer`（`^did:`） |

顶层 `additionalProperties: false`。**三套校验强度不同**，这是个坑：JSON Schema 最严
（校验 id 正则、时间格式、无额外键）；`src/validate.ts:24-51` 较松（**不查 `time.created`
格式**）；`src/bindings/file.ts:79-89` 最松（只查版本 + 四个非空）。

**L0–L3**：L0 = 能 parse+emit `*.ump.json`/`*.ump.md` 且导出时守 `consent.redact`；
L1 = L0 + 四操作 + 五种 kind + 一种 binding；L2 = L1 + `revise`/`forget` + 双时间 +
provenance + scope/consent 强制；L3 = L2 + `feedback`/`subscribe` + 签名验签 +
capability tokens + 抗注入再水化。**README 与 SPEC 对 L3 的表述不一致**（README 写
"contradiction relations"）。

**Conformance 有实质缺口**：`src/conformance.ts` 的 `runConformance()` 只探 HTTP 端点，
**从不校验 L0 文件产物**，也没有 JSON-Schema 校验器；**没有语言中立的独立向量目录**
（对比 portable-memory 的 `Conformance/`）。定级规则是 L1→L2→L3 连续全通过才升级。
⇒ 铁律 8 上，我们对 UMP 的自证合规只能"调它的 runner"或"自己校验 JSON Schema"。

**§5.3 抗注入再水化（读了 `src/rehydrate.ts` 全文 84 行）**，流水线
`Verify → Filter → Rank → Compress → Format → Frame`：
1. Verify：**若记录无 `integrity` 则无条件通过**——代码是 `return !requireValidSignature ? true : true;`，两个分支都返回 `true`。**`requireValidSignature` 对未签名记录是死代码**，不是"只丢未签名记录"。
2. Filter 可见性：默认 `maxVisibility="private"` ⇒ **默认只注入 private，shared/public 一律被丢**，除非调用方显式抬高。
3. Filter 墓碑：`status !== "tombstoned"`。
4. Rank：按服务器 `score` 降序稳定排序，`limit` 默认 12。
5. Format：输出显式标注不可信的块 `<ump:memory trust="untrusted-data">`，每条一行，前缀 `[<kind> project:<project>]`，正文截断到 `maxChars`（默认 500）。**实现里没有名为 "type enforcement" 的独立步骤**，类型体现为行首标签。
6. Content escaping：`sanitize()` 用正则删掉伪造的 `</?ump:memory[^>]*>` 标签，并把所有换行折叠成空格，保证「一条记录 = 一行」无法跳出围栏。
7. 返回 `{text, injected}`，便于调用方审计「到底注入了什么」。

**§2.9 admission and responsibility 是边界声明，不是新字段**。四条要点：
(a) 抽取/评审/审批属实现策略，服务器可要求该工作流；(b) **明确否证**——`remember` 成功、
`status=active`、`integrity` 签名都**不证明**记录真实/已评审/已批准，签名只证明"谁签的 + 字节没被改"；
(c) **`lifecycle.status=candidate` 是引擎-facing 的置信度提示，不是 consent/授权/待审状态**，
消费者可以召回 candidate，只有允许"确认后才复用"的系统才必须用 policy 执行这道门；
(d) admission 结果要随记录走就写 `provenance.actor`/`method`、引用签名的评审 artifact、
profile 细节放 `body.structured`，且**填完之后才签名**。
⇒ **UMP 自己就把审批排除在协议外**，我们的审批钩子（铁律 2/4）不能指望它承载。

**§6.3 Markdown projection 规范与实现不一致（重要坑）**：规范样例给的是类 YAML
front-matter；实现 `src/bindings/file.ts:44-66` 用的是**紧凑 JSON 块**——把记录去掉
`body.text` 后的全部元数据 `JSON.stringify(meta, null, 2)` 放在 `---\n…\n---\n\n` 之间，
反序列化直接 `JSON.parse(fm)`，不做 YAML 解析（文件头注释明说"so we avoid a YAML
dependency"）。⇒ **我们若写 `*.ump.md`，必须用 JSON front-matter**，用规范样例的 YAML
简写形式会让官方 `fromMarkdown()` 直接报错。另外 `toMarkdown()` 只处理单条记录，
**没有多记录 `*.ump.md` 的写法**。

**真实 `*.ump.json` 样例**（`examples/responsibility-confirmed.ump.json`，`did:key` 原文即遮蔽）：

```json
{"ump":"1.0","id":"urn:ump:scbkrconfirmedexample","kind":"semantic",
 "body":{"text":"The deployment checklist was reviewed and approved for reuse.",
   "structured":{"responsibility":{"profile":"…","status":"confirmed",
     "manifest_ref":"urn:scbkr:responsibility:deploy-checklist-2026-07-30"}}},
 "scope":{"owner":"did:key:****","project":"github.com/example/deployment","visibility":"shared"},
 "time":{"created":"2026-07-30T12:00:00Z","observed":"2026-07-30T11:55:00Z",
   "valid_from":"2026-07-30T12:00:00Z","valid_to":null},
 "lifecycle":{"confidence":1,"status":"active"},
 "provenance":{"actor":"did:key:****","actor_kind":"user","method":"responsibility_confirmed",
   "source":{"ref":"urn:scbkr:responsibility:deploy-checklist-2026-07-30"},
   "evidence":[{"ref":"urn:scbkr:responsibility:deploy-checklist-2026-07-30","weight":1}]},
 "consent":{"retention":"P365D","exportable":true},
 "integrity":{"content_hash":"blake3:jy4qfsgftbzitvwv3k7zcj5guxhyarti57cwp3rc7jqmqt2uo2aa",
   "signature":"ed25519:N5SkLyTQ…","signer":"did:key:****"}}
```

`integrity` 只覆盖"记录去掉 `integrity` 后 JCS 规范化 + blake3"（`src/integrity.ts:60-71`）；
签名是对 hash 字符串再 blake3 后 ed25519 sign；`signer`/`owner` 用 did:key
（multicodec `0xed01` + base58btc）。

**作为 Writer 目标的可写性**：能写出 L0 级、schema 合法的 `*.ump.json`/`*.ump.md`，
**L0/L1 不强制 DID 与签名**（§5.1 明说可先用 opaque `owner` 字符串，L2/L3 再升级）。
12 条必要条件中最容易踩的：id 必须是小写 base32 的 `urn:ump:…`（**要加一层可逆 id 桥接**，
官方 Recall adapter 就是这么做）；`*.ump.md` 的 front-matter 必须是 JSON。

**三条我们没有承载字段的铁律**：

| 铁律 | UMP 里有对应吗 | 处理 |
|---|---|---|
| 5 禁静默重嵌入 | **没有**。记录无 embedding 键；§8 把"是否携带向量与模型标签"列为 post-1.0 未决扩展 | 只能写 `body.structured` + 我们自己的报告。**不能声称 UMP 满足这条** |
| 6 禁静默衰减 | 无通用 unmapped-field 容器，只有 `body.structured` | 无法承载的字段显式写 `body.structured` + 报告；UMP 没有 AIMEM DNA 式强制保留语义 |
| 7 冲突不自动裁决 | `relations` 有保留词 `contradicts` | 冲突清单可写成 `relations[{type:"contradicts",target:"urn:ump:…"}]`；**但不要用 `revise` 的 supersede 语义表达冲突**，那等于自动裁决 |

**L0 的机器可验证性上限**：UMP **不给 bundle 级 checksum/manifest**（`*.ump.json` 就是
记录数组或 NDJSON），只有逐记录的 blake3+ed25519（且仅 L3）。`.well-known/ump.json`
只是发现清单，不含文件摘要。⇒ 我们"每步产出机器可验证凭证"只能靠**自己的迁移报告 +
逐记录 content_hash** 组合。

**推荐路径（推断）**：MVP 把 UMP 当 **L0 file binding 目标**（写 `*.ump.json` 数组 +
可选 `*.ump.md`），`owner` 先用稳定 opaque 字符串，`provenance.actor_kind="import"` +
`provenance.source.provider` 标源，把 embedding/模型/丢失字段/冲突清单全部塞
`body.structured`；等真需要 L2/L3 再引入 did:key + 签名 + capability token。

**M5 落地**：`writer-ump` 已实现 `records.ump.json` 数组 Writer，
官方 schema/license 原样 vendored 自 commit `5defe7839dd09da255744c24b0166e600e8e56cd`，
见 `crates/writer-ump/schema/README.md`。离线 schema 校验与 metadata 往返通过；
不实现签名、DID、UMP 服务端或 L1–L3。源创建时间缺失时明确使用目标迁移创建时刻，
非空 `consent.redact` 无法处理则拒写，不仅在输出中抄字段。

**`src/importers/filesystem.ts` 细节**（205 行）：只做本地 Markdown → UMP draft。
识别 `CLAUDE.md`/`AGENTS.md`/`MEMORY.md`/`context.md` 与 `.md/.mdx/.markdown`；
按 H1–H3 切段；`inferMemoryKind()` 按文件名/heading 关键词猜 kind；给
`lifecycle.status="candidate"`、`confidence` **硬编码 0.55/0.7/0.75**；
`provenance.method="filesystem:<kind>"`；**默认 `maxFileBytes=512_000`，超限静默返回 `[]`**。
`adapters/recall/` 是纯映射（含 `urn:ump:<base32>` ↔ uuid 可逆桥接），声明 Recall 端
当前是 **L2**，L3 需在 binding 层接 capability token。

### AIMEM — Memory Interchange Bundle Format

- IETF draft: https://www.ietf.org/archive/id/draft-vu-aimem-bundle-00.html
- 参考实现（**404，不存在**）: https://github.com/aimem-protocol/aimem-reference

作者 Vu Duc Minh（MemoryAI，越南），Independent Submission，Informational，
2026-06-14 发布，2026-12-16 过期。vendor-neutral 自包含 JSON bundle
（chunks / edges / entities / chunk_entities）。硬语义最多的一份。

已从原文逐条核实的硬约束：

- DNA-class = `preference` / `decision` / `identity` / `pitfall` / `procedure`
  五类，**不得静默衰减、删除或取代**；`is_pinned` 共享同一不变式。
- `memory_type` 枚举：fact / preference / decision / identity / pitfall /
  procedure / episodic / goal。`zone` 枚举 critical / important / standard。
- embedding 为 base64 little-endian float32；**envelope 必须声明
  `embedding_dim` + `embedding_model`**（注意是信封级，不是 per-chunk）。
  Consumer 不支持该模型时 MAY 丢弃 embedding 并**必须**产出结构化警告，
  **MUST NOT 静默重嵌入**。
- checksum = 移除 checksum 字段后按 JCS（RFC 8785）规范化 + SHA-256，
  Consumer 必须校验，失败 409。
- `content_hash` 为 `sha256:<hex>`，导入时必须校验，不匹配则拒收该 chunk（422）。
- 幂等键是 URN `(producer, local)`；`created_at` 相同但 `content_hash` 不同必须
  报冲突；`created_at` 更新时拒绝或更新由实现自选但**必须写进文档**。
- 擦除（GDPR A17）：硬删 + 级联 + 递减共享内容池引用计数 + 过滤后续导出，
  审计日志只记「发生过擦除」不记「擦了什么」。
- 签名用 COSE_Sign1 分离签名，公钥可放 `/.well-known/aimem-pubkey`。
- 兼容旧值 `"memoryai-bundle"`；媒体类型 `application/aimem-bundle+json`，
  扩展名 `.aimem.json`；超 16 MiB 建议 NDJSON 流式变体。

**注意**：草案 §4 说「测试语料随参考实现分发」，但该仓库 404。也就是说
AIMEM 目前**没有可运行的 conformance 语料**，我们只能按文本实现。
这条要记进迁移报告的「证据等级」。

### MIF — Modeled Information Format

- Repo: https://github.com/modeled-information-format/MIF
- Docs: https://mif-spec.dev/
- OKF 合规超集决策: https://github.com/modeled-information-format/MIF/blob/main/adr/ADR-009-okf-compliance-superset.md

v1.0.0 已重定位为「OKF-compliant opinionated knowledge content model」，
**AI memory 降级为它的第一个 profile**（`profiles/ai-memory/`），不再是身份本身。
Markdown 是 canonical，JSON-LD 是派生投影（ADR-011）。21 条 ADR 里值得读的：
ADR-008 decay model rationale、ADR-009 OKF superset、ADR-013 provenance
lightweight core + optional prov layer、ADR-021 container profile。
有 `scripts/migrate_0_1_to_1_0.py` 与 `scripts/okf_validate.py` 可跑。
配套 `mif-rs`（mif-core / mif-schema / mif-ontology / mif-cli / mif-mcp）。

靠「填别人的空」活下来的实证，也是本仓库定位策略的先例。

### OKF — Open Knowledge Format

- Repo: https://github.com/GoogleCloudPlatform/open-knowledge-format
- Spec 镜像: https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md

纯 Markdown 目录信封，本地层最大的开源入口之一。MVP 的 Markdown Writer 即对齐此形状。

SPEC 已核实的结构：§3 bundle structure + reserved filenames、§4 concept documents
（frontmatter + body）、§5 provenance `sources` / trust `generated`+`verified` /
trust tiers / lifecycle `status` / `stale_after`、§6 cross-linking + `references/`
约定、§7 actor convention、§8 index files、§9 log files、§10 attested computations。
`bundles/` 下有真实样例（acme_retail / crypto_bitcoin / ga4 / stackoverflow），
`src/` 是 Python 包，`tests/` 有验证器。**要抄 envelope 就直接读这些样例。**

#### 实测发现（2026-10-05，读 `lab/upstream/open-knowledge-format/SPEC.md`）

本仓库默认的「家」用它（[design.md](design.md) DEC-19），所以记下与此相关的硬事实：

- 当前是 **v0.2**（§12）。bundle 根 `index.md` 可声明 `okf_version: "0.2"`，这是 `index.md` 唯一允许的 frontmatter。
- **合规门槛极低**（§11）：每个非保留 `.md` 有可解析 YAML frontmatter，且含非空 `type`，就合规。
  Consumer MUST NOT 因缺可选字段、未知 `type`、未知 frontmatter 键、断链、缺 `index.md` 而拒收。
- **扩展**（§4.1）：Producers MAY 加任意键；Consumers SHOULD 往返保留未知键、MUST NOT 拒收。
  （对照：Memanto 的 `okf_loader` 把未知键塞进正文尾部并截断，属于没守 SHOULD。）
- Concept ID = 文件在 bundle 内的路径去掉 `.md`（§2），所以路径就是身份，不该编码可变属性。
- 溯源 `sources[]`（`resource` 必需，`id`/`title`/`author`/`usage_count`/`last_modified` 可选，§5.1）；
  `generated: {by, at}` 记谁写的、何时有实质改动；`verified: [{by, at}]` 记谁对照来源确认过（§5.2）；
  信任等级由 `verified` 推出，`human:` 前缀的 actor 才算 human-reviewed（§5.3、§7）。
- 生命周期 `status: draft | stable | deprecated`（缺省 stable，§5.4）；`stale_after` 是绝对时刻（§5.5）。
- `log.md`：按 `YYYY-MM-DD` 分组、新的在前的变更记录（§9）。推荐用 git 分发（§3）。
- **v0.1 → v0.2 有破坏性变更**（§13.1）：`timestamp` 被 `generated.at` 取代（Consumer MAY 回退读旧字段）；
  正文 `# Citations` 被 `sources` 取代。MIF 的 ADR-009 pin 的是 v0.1。

### OMPI — Open Memory Protocol Initiative

- Repo: https://github.com/The-AI-Disclosures-Project/Open-Memory-Protocol
- Hub: https://ai-disclosures.org/ompi
- 治理: Mozilla、IBM 为正式 in-kind 伙伴；Letta 为草案技术协作者；Block/goose 为实现协作者

**不是一份规范，是一个提案家族**（2026-10-02 仍在动）。`early-draft-specs/` 现存：

- `draft-v0.2-packer.pdf` — Letta/Packer，6 页，目录层级 + 选择性加载
- `draft-v0.1-aidp.pdf` — AI Disclosures，内部标题 Federated Memory Protocol
- `draft-v0.1-ibm.pdf` — IBM，导入导出身份 / 溯源 / 失效 / 权限映射
- `draft-v0.1-cognee.md|pdf`、`draft-v0.1-contextnest.md` — 后补的两份提案
- `ams-card.md` — Agent Memory System Card（Mila + Mozilla 贡献）
- `scoping-note-2026-09.pdf`

v0.1 文本草案的四条 harness 加载契约（窄但有约束力）：根目录 `.md` 进上下文 →
子目录 `.md` 延迟 → harness 必须让 agent 知道延迟记忆存在并可发现 →
支持按需选择性读取。每个参与目录应含 `MEMORY.md`。草案**故意**把存储后端、
同步、版本控制、生命周期/所有权策略排除在外。

`prototypes/python-loader-validator`（实验性 v0.1 校验器，明确不含存储/同步/
上下文预算）+ `prototypes/acp_memory_server`（旧实验，索引多 harness 会话）。

`prior-art/` 六份文档（academic-foundations / agent-harness-memory-systems /
convergence-analysis / emerging-standards / enterprise-memory-services /
interoperability-protocols）与 `research/the-memory-walled-garden` —— 与我们的
[memory-products.md](memory-products.md) 直接重叠，尤其 `convergence-analysis.md`
的「大家一样的地方 / 分歧的地方 / 最小可行标准（memory object schema + exchange
format + operations interface + consent signal + provenance requirements）」。

#### 实测发现（2026-10-05，读 prior-art + 草案）

**8 条分歧轴**（`prior-art/convergence-analysis.md`），每条给出「谁在用哪些取值」：

| # | 轴 | 取值分布 |
|---|---|---|
| 1 | 记忆类型分类法 | 无类型（Codex/Cursor/Aider/Gemini CLI）· 实用标签（fact/preference/instruction）· 认知科学四类（episodic/semantic/procedural）· 三层（core/recall/archival，Letta）· 行为类（MemoryHub） |
| 2 | 存储后端 | 纯文件 · 关系库 · 向量库 · 知识图谱 · 多后端 · 托管云 |
| 3 | 图结构 vs 扁平记录 | 有类型关系（derived_from/supersedes/conflicts_with，Zep/Cognee）vs 独立记录 |
| 4 | 检索策略 | 启动全量加载 · 关键词/全文 · embedding · 混合 · 相关度排序 |
| 5 | 用户可见性与控制 | 全可见（harness）· API+看板（企业）· 看/删限改（ChatGPT）· 极简或黑箱（Pi） |
| 6 | agent 著 vs 用户著 | 用户文档（Codex/Cursor）vs agent 自主（Claude Code/Goose）vs LLM 抽取（企业） |
| 7 | 记忆生命周期 | 无 · TTL · 语义过期 · 衰减策略 · 软删+保留 · 版本+取代 · 矛盾检测 |
| 8 | 可移植性 | 纯文件（天然可移）· COGX JSON · PAM JSON/CBOR+Merkle-DAG · 导出导入 API+冲突解决 · **无可移植性（AWS/Google/Microsoft/Mem0/Zep/ChatGPT/Pi）** |

第 8 条是它的核心论点：**多数服务型系统根本没有可移植机制**，这就是 OMPI 存在的理由。

**「最小可行标准」六节**（含每节要求字段）：

1. **对象 schema 必需字段**：唯一标识、Content、Scope（personal/project/extended）、**Scope qualifier**（哪个项目/哪个用户）、Owner、时间戳（created/modified）、Provenance origin（user/agent/system/import）。
2. **对象 schema 推荐字段**：记忆类型（推荐词表）、importance/salience、tags/labels/domains、版本号、过期/TTL、可扩展 metadata。
3. **交换格式**：两种形态须是同一逻辑对象的**无损表示且可往返**——Markdown+YAML frontmatter（文件式）与 JSON（API 式）；外加 **bundle 格式**，含 **manifest（源系统 / 导出时间 / schema 版本）** 与**冲突解决语义（skip / overwrite / merge）**。
4. **操作接口**：Write/Read/Search/Update(带版本)/Delete(硬删以合规)/Export/Import(带冲突解决)；至少两个 profile：**File profile**（目录布局、命名、索引约定）与 **Tool profile**（MCP 工具名与输入输出 schema）。
5. **同意信号**：可按 scope 查询/设置的 `memory_enabled`；scope 关闭时不得写新记忆。
6. **溯源要求**：最低 = 谁创建 + 如何创建；完整溯源（lineage、confidence、模型身份、attestation）为推荐；跨信任边界的验证交给信任层（PTC 等），**不属记忆协议**。

**与 [memory-products.md](memory-products.md) 对读出的盲区**（左 = OMPI 有，右 = 我们现状）：

| OMPI 的轴/字段/机制 | 我们的覆盖 | 判定 |
|---|---|---|
| **Scope qualifier**（"哪个项目/哪个用户"的独立字段） | 只有「身份字段八套八个答案，只能由管道赋值」，未立为独立列 | **真盲区** |
| **同意信号 `memory_enabled`**（per-scope 开关，关闭即禁写） | 完全未涉及 | **真盲区** |
| **bundle manifest**（源系统 / 导出时间 / schema 版本） | 无；只谈迁移报告 artifact | **真盲区** |
| **冲突解决语义词汇 skip/overwrite/merge** | 有铁律 7「只聚类不裁决」，但**没给目标侧 import 的冲突动作词汇**，也未处理「merge 与不裁决如何兼容」 | 需补口径 |
| **内容寻址身份 / checksum**（PAM BLAKE3、Context Nest SHA-256 链） | 仅提到 Panella `content_sha256`，未上升为规律 | 部分覆盖 |
| **Tombstone / 导入防复活** | 无；只有 Panella「未批准即不可见」 | **真盲区**（对上铁律 6） |
| **valid time vs record time 作为一等字段对** | 有 Graphiti 双时序实测，未泛化成报告字段对 | 部分覆盖 |
| **auditable flag**（可追溯到问责的人） | 有 Panella 审批、Memoria 审计链，无「auditable 标记」概念 | 部分覆盖 |
| **逐对象导入回执**（accepted/transformed/omitted/unresolved/rejected + 身份映射 + 原因） | 迁移报告方向一致但粒度更粗 | 需吸收 |
| **credentials 排除于交换之外** | 有 PII/密钥出站闸门 | 已覆盖（外部验证） |
| **scope tags → ACL 映射，scope 由系统设置** | 铁律 4 立场一致（owner 客户端自声明不可信），但它把 scope tag 绑到 ACL | 立场一致、机制可借鉴 |
| **federation / 身份透传** | 未涉及 | **真盲区** |
| **conformance floor + profiles** | 铁律 8 只有「套件不 import 实现」，未定义 floor 与 profile 分层 | 需补 |
| **dangling links on export / copy vs move vs federation / forget 传播** | 未涉及 | **真盲区** |

**逐对象导入回执的 5 态**（`accepted` / `transformed` / `omitted` / `unresolved` /
`rejected` + 身份映射 + 原因）是 OMPI 比我们更细的地方，值得直接吸收进迁移报告 schema。

**各草案差异**（`early-draft-specs/`）：

| 草案 | 作者/组织 | 页数 | 核心主张 |
|---|---|---|---|
| `draft-v0.2-packer.pdf` | Charles Packer / **Letta** | 6 页 | `MEMORY.md` + 目录层级 = 渐进披露；四规则 harness 契约（见下） |
| `draft-v0.1-ibm.pdf` | **IBM**（Gabe Goodhart） | 约 2–3 页 | source-local identity / human+AI provenance / scope→ACL / 不可变历史（见下） |
| `draft-v0.1-aidp.pdf` | AI Disclosures Project（Sruly Rosenblat），内部标题 **Federated Memory Protocol** | 未精读 | 多记忆服务器背后一个插件，`/fmp/*` 端点；三类型 ground truth / transcripts / inferences |
| `draft-v0.1-cognee.md` | Cognee（Vasilije Stepanovic） | 全文已读 | 六部分语义核心；COGX 作工作台；Markdown 降为 profile，runtime/federation 为 binding；**对每份草案逐一点评** |
| `draft-v0.1-contextnest.md` | PromptOwl | 全文已读 | 八个 frontmatter 键 + 散文链接；`contextnest://` 寻址；哈希链版本 + checkpoint；forget 协议 |
| `ams-card.md` | Mila + Mozilla | 卡片 | 记忆系统的结构化**文档**格式，性能与隐私分轴评分；**只做披露不做交换** |
| `scoping-note-2026-09.pdf` | Strauss & Rosenblat (AIDP) | 未读 | 「窄腰 = 可移植记录 + consolidation lineage」的来源 |

**IBM 草案对上了我们的迁移报告字段**（这是最相关的一份）：

- **身份**：`Memory Identifier` **在来源系统内唯一**，无全局 ID。
- **溯源**：`Author`（参与创建的人类）+ `AI usage / Provenance`（用了什么 AI、怎么用，可对应 EU AI Act）+ 时间戳。
- **失效/时间**：`Creation timestamp` + `Invalidation timestamp`（均 ISO 8601）；**记忆不可变，无 update 时间戳**，逻辑变更靠 **override + invalidation** 以支持"时间旅行"；`Versioning` 语义留实现，但**必须有标准 comparator**（数值序号/语义版本）。另有可选 `Source Material` 链回原始信息。
- **权限映射**：`Semantic Tags`（标准只定义 tag set 的**形状**、不定义值，建议 JSON-LD `@context` 承载词表）+ `Scope Tags`（定义 scope 的任意标签，**值不定义**，**将绑定到 ACL 策略**）。草案自己承认「semantic 与 scope tags 的不连续可能导致导出/导入需要映射步骤」。
- **运行时四动词**：`remember`（主动调用）、`recall`（工具调用，**系统必须校验请求 scope 是否在 ACL 内**）、`observe`（hook，只读）、`decorate`（hook，可变）；**scope tags 由系统加，不由 agent 定义**。
- **导入导出**：schema 明确定义，但**一次只在一个系统有效**——导入新系统后成为该系统的新记忆，在旧系统不再有效。Cognee 批评此规则"比可移植性所需更窄"。

**Letta v0.2（Packer）的加载契约**（Markdown Writer 的形态规范）：

- 形态：顶层 `MEMORY.md` + 附加 markdown 文件与子目录；**层级 = 文件层级**，顶层是 agent 最该知道的，**子目录 = 渐进披露机制**。
- 四规则：① 顶层 `.md` 至少部分常驻上下文（可截断）；② **根以下 markdown 不自动载入**；③ 若存在嵌套，harness **必须让 agent 知道直接子目录有延迟内容**（写在 `MEMORY.md`）；④ agent 能用文件工具按需加载单个延迟文件。
- 尺寸建议：单顶层文件约 500 tokens（约 20,000 字符），上下文记忆总量约 20,000 tokens。
- **必须存在顶层 `MEMORY.md`**，否则非法（PDF 给了反例树）。
- 可选元数据：markdown 头里 `description` 与 `metadata`（如 `session_evidence`、`read_only`）。
- 明确 out-of-scope：git 跟踪、长度强制、存储机制、非 markdown 文件；**不定所有权/写权限/同步/版本控制**。
- 定位（推断）：这是**加载形态规范**而非交换格式（Cognee 明确指出文件深度无法可靠承载 identity/provenance/policy/lifecycle），可作我们 Markdown/OKF Writer 的加载侧约束。

**`research/the-memory-walled-garden/the-memory-walled-garden.md` 一句话**：用户记忆被锁在
第一方应用（ChatGPT/Claude）的围墙里——memory profile 尚可经第三方 MCP 服务器搬运，
但 **chat history 搬不走**，五家主流记忆 MCP 服务器没有一家能读它们的会话历史；
出路是 OpenAI/Anthropic 把记忆暴露成 MCP 服务器/API 且支持动态同步与授权，
否则第三方只能依赖显式记忆、形成两层市场。

**另注**：`ams-card.md` 本地文件**不含字段结构**，只有一个指向
`github.com/mila-iqia/agent-memory-system-card` 的外部链接，要列字段须另取该仓库。

### MacPaw portable-memory

- Repo: https://github.com/MacPaw/portable-memory
- Swift: https://github.com/MacPaw/portable-memory-swift
- 论文: https://research.macpaw.com/publications/portable-memory

MacPaw（CleanMyMac 那家）出的 vendor-neutral 格式与协议。卖点是
"lossless, local-first, with verifiable deletion"。SDK 0.3.0 / format 1.1.0 分立
（manifest 里的 `format` 与包版本独立演进），已有 RFC-0001/2/3 流程，
`pip install portable-memory`，`brew install macpaw/taps/portable-memory`，
CLI 叫 `mem`（`mem inspect`）。

18 份 JSON Schema：category / chunk / community / context / core / edge / entity /
episode / episodeLink / fact / factLink / log / manifest / preference / procedure /
resource / **secretRef** / **tombstone**。`Conformance/` 有 fixtures + vectors，
`Spec/rfcs/` 有 RFC。

`secretRef` 与 `tombstone` 两个 schema 值得单独看：前者对上我们的 PII/密钥出站闸门，
后者对上「可验证删除」。

#### 实测发现（2026-10-05，读 Schemas + Spec + Conformance）

**`secretRef`（`Schemas/secretRef.schema.json`，落 `items/secretRef.jsonl`）**：
描述原文 *"a reference to a vault secret. Carries encryption metadata only; NEVER the
plaintext or ciphertext"*。字段：`id`（必需，`sec_…` 前缀）/`label`/`sensitivity`/
`category`/`preview`（**仅遮蔽提示**，如 last-4，明写 "MUST NOT contain recoverable
secret material"）/`encryptionMetadata`（**值如何被保护**，不是值本身）/`createdAt`/
可选 `lastAccessed`；`additionalProperties: true`。

配套：`episode` 记录有必需的 `vaultRefs: string[]`，即「这条记忆引用了哪些密钥」。
导入侧（`importer.py:249-260`）只恢复 metadata skeleton，并往
`MemImportReport.warnings` 写 *"metadata skeleton restored, but the encrypted VALUE is
not in the bundle — transfer it via an authorized encrypted channel"*。

⇒ **关键结论：`secretRef` 是「引用外部密钥」，不是「脱敏后内联」。** 明文与密文都
NEVER 随包走。**因此它不能当 PII/密钥闸门的落点**——它承载不了"我把某段 PII 脱敏内联了"
的语义。**我们的闸门必须放在 Reader 出口 / 进 canonical model 之前**，绝不让真实密钥/PII
进入任何 `text` 字段（UMP 的 `body.text` 与 portable-memory 的 `episode.summary`、
`chunk.text` 都一样，写进去就是明文泄露）。源系统本身有密钥条目时，正确做法是只输出
一条 `secretRef`（或 UMP 侧走 `consent.redact` JSON-path），并在报告里记"值未随行"。
UMP 的对应机制是 `consent.redact`（JSON-path，跨可见性边界时剥离）与
`consent.exportable:false`（整条不导出），同属出站闸门，也不是内联脱敏存放点。

**`tombstone`（`Schemas/tombstone.schema.json`，落 `audit/tombstones.jsonl`）**：
描述原文 *"a first-class, monotonic deletion record applied FIRST on import. Carries
proof-of-reach: every derived artifact removed."* 字段：`id`/`op`（`delete`|`redact`）/
`targetKind`/`targetID`/`deletedAt`/可选 `reason`/`actor`（**不透明** id，规范注释
"keep PII out of the portable trail"）/`derived`（**proof-of-reach**，含
`sentenceIDs`/`factIDs`/`edgeIDs`/`entityIDs`/`chunkIDs`/`episodeLinkCount`/
`embeddingCacheKeys`）/可选 `signature`（ed25519，L3）。

⇒ **关键结论：`delete` 是「内容物理删除 + 永久留墓碑」，不是软删除标记。**
- 内容真删：移除 target 与**一切派生物**——FTS 行、稠密向量、content-hash 键的
  embedding cache、句子索引、仅由它支撑的图边、被这些边孤立的实体、A-Mem 链接、
  访问历史、同步副本。
- 墓碑永久且单调（"cannot be un-seen"），`reason`/`actor` 会随包长期流转（规范专门提醒
  别把个人数据写进 `reason`）。
- **删除后不再参与检索**：导入顺序是 normative 的
  「验证 → **先应用 tombstones** → 按 key merge → 重新派生本地产物 → 收敛副本」，
  且 `No resurrection` 规则适用于**每一种 kind**。
- L2 badge 的门槛就是"删除后经由**每一条**检索路径都不可达 + 有墓碑"，
  探测路径：dense vector / lexical / k-hop graph / embedding cache / bi-temporal `as_of` /
  synced replica。
- 注意 **proof-of-reach 是"删除方声明的清单"，默认不是密码学可验的**（只有 L3 的
  `signature` 让它可验来源）。

**两个格式的删除语义相反（对我们取舍有用）**：UMP `forget` 默认只把
`lifecycle.status` 改 `tombstoned` 并**保留整条记录**（内容还在），`hard:true` 也只是把
`body.text` 换成 `[erased: reason]`、记录仍在。portable-memory 是**物理移除内容 + 留证据**。
⇒ 我们的删除语义映射**必须分叉记录**，不能共用一套。

**Conformance（`Conformance/`）**：`fixtures/` 有 `sample.mem`（format 1.0 向后兼容）、
`sample-1.1.mem`（Python SDK 写出，跨 SDK 互验）、`transfer/`（TransferTextAdapter 字节级 pin）；
`vectors/` 有 `canonical-json.json`（**26 条向量**，覆盖整数值浮点 `1.0→1`、`-0.0→0`、
最短往返小数、`2^53`、`2^53+1`、`Int64.max`、`UInt64.max`、非 ASCII 原样、控制字符
`\u00xx` 小写、键排序 BMP、嵌套递归排序、数组顺序保留等）、`signed.mem/`（已签名完整
bundle）、`signing-test-key.json`（ed25519，私钥来自固定种子 `00 01 … 1f`，非真实秘密）。

**签名怎么验**：Ed25519 对 **Canonical JSON 字节**签名；token 线格式
`ed25519:<publicKeyHex>:<signatureHex>`（均小写，公钥 32 字节 raw、签名 64 字节 raw），
公钥随 token 走，**只有在验证方 trusted 集合里存在该公钥时才可能通过**（用不受信钥匙
自签换包会被拒）。bundle 签名 `manifest.sig` 是对**磁盘上 `manifest.json` 逐字节**的
detached 签名，因 manifest 列出每个文件 sha256，一个签名就间接认证整包。
tombstone 签名是对"**去掉 `signature` 字段后的** Canonical JSON 字节"签名，
**未签名的墓碑一律视为无效（fail closed）**。验证失败语义：配了 trusted 钥匙时
`manifest.sig` 缺失/畸形/非受信 → **拒绝整包**；未配 → 忽略签名只查完整性。
`manifest.sig` **被排除在字节可复现保证之外**（签名不要求跨实现字节一致）。

**Canonical JSON 规则**（规范 §1.1 + `portable_memory/_codec.py`）：UTF-8 无 BOM；
JSONL 一行一对象、记录内无空白；键按 **Unicode code point 升序递归**排序；
字符串 `/` 不转义、非 ASCII **原样 UTF-8**；数字走最短往返十进制（`1.0→1`、`-0.0→0`，
`|x| < 1e16` 的整数值浮点转 int，≥1e16 走指数）；**缺失可选字段必须省略、绝不写 null**；
时间戳 RFC 3339 UTC、字面量 `Z`、**整秒**；NaN/Infinity 拒绝；**外来 kind 逐字节保留、
豁免再规范化**以保证 checksum 稳定。

**明确声明的跨实现不保证项**（`vectors/README.md`）：BMP 之外的 JSON **键**
（UTF-16 code unit vs code point 排序差异）、整数量级 ≥1e16、超 `UInt64.max`、
非 ASCII id 的**行内排序**。**另注**：>2^53 的整数字段需要 bigint 感知的 JSON 解析器，
JS 的 `JSON.parse` 会静默丢精度——**若我们用 TS 写 Writer 这是实际风险点**。

**对上铁律 8（套件不 import 实现）——判定：数据独立、执行器不独立**：

| 层面 | 结论 |
|---|---|
| oracle 数据（`canonical-json.json`、`signed.mem`、`signing-test-key.json`、`fixtures/`、`Schemas/*.json`） | **独立**。纯数据 + JSON Schema，不 import 任何实现；README 明说非 Python 实现者可以只靠 `Schemas/` 校验；且这些文件在 **Swift 与 Python 两个仓库里字节相同**，存在第二个独立语言实现交叉验证（比 UMP 强得多） |
| 执行器 | **不独立**。`BundleValidator` 是 SDK 的一部分，向量加载测试直接 `from portable_memory import …`，`vectors/generate.py` 也 import SDK |
| L2 探测 | **不在套件里可执行**。README 明确 live all-routes 断言"由 adopter 对自己的检索栈完成"，套件只提供 offline 部分 |

⇒ 我们能用的是它的**数据 + Schema**，不能用 `BundleValidator`；且 L2 那一层它自己也没给
可执行的独立判定器。

**format 1.1.0 改了什么**：版本是**两条独立线**（SDK `0.3.0` vs on-disk `format 1.1.0`）。
1.1.0 **只加了 4 个可选 manifest 字段**，1.0 bundle 仍合法：
① `specURL`；② `coverage:{from,to}`（bundle 内 episode 的 `eventTime` 最早/最晚）；
③ `scopes: string[]`（引用到的 scope/context id 排序去重集合）；
④ **`bundleDigest`**（`CHECKSUMS` 文件逐字节的 sha256，校验方发现存在就必须从磁盘重算，
不匹配即失败）。`bundleDigest` 补充而非替代 `manifest.sig`：**digest 让哈希"一致"，
签名才让它"可信"**。

**三条尚未进正式版的 RFC**（目标 format 1.2.0，评议期至 2026-10-05，**MVP 不能依赖**）：
- **RFC-0001 scopes & visibility**：`visibility {level: private|group|shared|public,
  principals: [...]}` 可挂 context 与各 kind；优先级 `record > 最近祖先 context > 未指定`；
  **未指定时默认收紧、绝不放宽**（1.0/1.1 的沉默不得解释为 public）；合并时取更严级别 +
  principals 交集。明确非目标：不做强制、不做身份联邦、不做逐字段 redaction。
- **RFC-0003 provenance typing**：加三个可选字段 `claimClass`
  （`asserted`/`observed`/`inferred`/`merged`/`imported`，开放枚举、未知值必须保留）、
  `assertedBy`（`<type>:<id>` principal）、`mergedFrom: string[]`；与 W3C PROV 对齐
  （`assertedBy ≈ prov:wasAttributedTo`、`mergedFrom ≈ prov:wasDerivedFrom`）；
  明确**不做**信任评分/真值裁决。
- RFC-0002（`ext` 扩展到所有 kind）未读全文，是 RFC-0001 的前置依赖。

⇒ 对我们的映射价值（推断）：RFC-0003 的 `claimClass`/`assertedBy` 正好对应我们的
"来源可信度 / 是谁断言的"，RFC-0001 的 visibility 对应"这条记忆的可见范围"。

### memcommons/spec — Memory Commons

- Repo: https://github.com/memcommons/spec

2025-07-12 的 v0.1-draft。**要修正一条旧判断**：它不再是「只有规范没有桥」——
`IMPLEMENTATIONS.md` 已有 1 个登记实现（Mnemoverse Rooms，CP-1 已验证、
CP-2–CP-7 claimed run pending），并附 `test-vectors/`（addressing / lifecycle）
与参考解析器 `tools/mspace_reference_parser.py`。G4 中立承诺需两个独立组织的
全量 verified 条目才触发。

但更关键的是：它 **§4 明确把 consistency / wire schema / discovery+federation
划出 v0.1**。也就是说它规范的是 **mspace 寻址语法 + 权限生命周期 + 错误分类 +
provenance-on-read**，不是交换格式。**因此它不是我们的映射目标**，
只是寻址/权限语义的参考。引用 Mnemoverse 五面互操作分解，参考列表含
OWASP ASI06、UMP、PAM。

## 平台原生导入通道

各家平台自己开了记忆导入，是零成本的 Writer 目标（不需要我们写文件格式，
只需要产出可粘贴文本或可上传的 zip）。

### Claude（Anthropic）

- 文档: https://support.claude.com/en/articles/12123587-import-and-export-your-memory

路径：Settings > Memory > Start import。流程是**粘贴一段文本**，Claude 自己抽取成
独立 memory entries。官方给的导出 Prompt 要求格式 `[date saved, if available] -
memory content`，且明确要求「preserve my words verbatim」「Do not summarize,
group, or omit any entries」。

新版（Settings > Memory）与旧版（Settings > Capabilities > Memory）并存，
旧版导入后「24 小时内生效」、可 `Manage edits` 审查。**官方自述
"experimental and still in active development"，且「可能不保留与工作无关的个人细节」**
——这是目标侧静默衰减的活样本，我们的迁移报告必须预警这一类。

### Gemini（Google）

- 文档: https://support.google.com/gemini/answer/16868299

两条通道：① **记忆**同样是粘贴文本，Gemini 提供 Prompt 让你去源平台取，
粘回后点 Add memory，会新建一个 chat thread；② **聊天记录**是上传 `.zip`，
上限 5 GB，每天最多 5 个，**官方文档直接教怎么从 ChatGPT（Settings > Data
controls > Export）和 Claude（Settings > Privacy > Export）导出**，
即 Gemini 能吃这两家的官方导出包。

限制：仅个人 Google 账号（不支持工作/学校/受监管账号）、18 岁以上，
**EEA / 瑞士 / 英国不可用**。导入的聊天计入 Activity 并可能用于训练。

### ChatGPT（OpenAI）

**目前没有从其他平台导入记忆的官方通道**（导出有，导入无）。搜索结果里出现的
"ChatGPT memory import" 基本是浏览器扩展（MemoryPlugin）或第三方博客。
所以对 ChatGPT 方向我们只能做 Writer（写进它的记忆），不能做 Reader 的对应侧。

## 工程模式参照

### A2M — Agent2Memory Protocol

- Repo: https://github.com/dibenedetto/a2m-protocol

draft 0.1。JSON-RPC 2.0 over stdio/HTTP，必需 core + 可选 capabilities 分离，声明了
capability 但不支持算协议失败。conformance 套件（tools/conformance.py）不 import
被测实现——本仓库适配器注册表与 conformance 套件的样板。规范自己警告 `owner` 是
客户端自声明的，网络场景必须由认证主体推导。

## 治理后端（可插拔，本仓库不自建）

### Panella

- Repo: https://github.com/panellatech/panella
- Site: https://panella.tech/
- PyPI: https://pypi.org/project/panella/

default-deny 的 self-hosted MCP memory server：写操作是提案，须人批准后才落盘；
批准记录进 tamper-evident 哈希链；agent 永远拿不到 approval credential。本仓库
approval receipt 钩子的参照与目标后端。注意其 `memories.content_hash` 列实际存的是
审批 `durable_id`，不是内容哈希——命名陷阱。

### Memoria

- Repo: https://github.com/matrixorigin/Memoria

MatrixOne 之上的 CoW 引擎，Git for memory：snapshot / branch / checkout / rollback /
merge，全审计链，MCP server，docker compose 部署。本仓库快照钩子的参照与目标后端。

## 竞品标尺

### Memanto

- Repo: https://github.com/moorcheh-ai/memanto
- Docs: https://docs.memanto.ai/
- 商业后端: Moorcheh（hosted 检索需 API key，有 free tier）

Moorcheh 的商业记忆产品，迁移工具链开源。`memanto migrate`（dry-run + 节省报告）、
`memanto memory export --okf`、`memanto migrate okf ./bundle`（宣称 unmapped
fields 保留、无损）。2026-10-03 仍在动，v0.2.x。

**核心支持的源**（`memanto/cli/migrate/mappers.py`）：`map_mem0`、`map_letta`、
`map_supermemory`、`map_okf`、`map_zep`、`map_hindsight`，另有 langfuse 独立模块。
**ChatGPT / Claude 导出不在核心**，只在 `examples/migrations/chatgpt-claude-okf`。

`examples/migrations/` 实际只有 6 个目录（9-migration-adapters / antigravity-brain /
chatgpt-claude-okf / hermes-holographic-to-okf / langmem / mcp-memory-server）——
bounty 收了 46+ 个 PR 但绝大多数未合入，说明其社区适配器质量参差。

**它的空白正是我们的位置**：`memanto/app/services/okf_export_service.py` 把私有元数据
塞进 frontmatter 的 `x_memanto` 块（`x_memanto["type"]` 等）；dry-run 报告是打印文本，
不是带 schema 的 artifact；无 PII/密钥闸门；无审批钩子；bounty 的验收矩阵里
也没有这些项。

#### 实测发现（2026-10-05，读 mappers.py + okf_export_service.py + migrate CLI）

**三条所有 mapper 共用的损失机制**：
1. 无法入 schema 的字段塞进正文尾部 `[Supporting data]` 文本块，**总长上限 800 字符、
   单值上限 200 字符**（超长截断加 `...`）。即「保留」= 降级为可检索纯文本，**且会被截断**。
2. `updated_at` 一律写成迁移时刻（`_now_utc()`），**除 okf 外源侧 `updated_at` 全丢**。
3. `confidence` 除 okf 外**硬编码 0.8**（supermemory chunk 0.7）；`title` 除 okf 外
   一律由 content 截断派生。

**逐 mapper 的丢弃清单**：

| mapper | 明确丢弃/降级 |
|---|---|
| `map_mem0` | `expires_at` 只写进 footer 文本，**row dict 里没有该键**（算出但未落键）→ 不持久化；`metadata`/`score`/`hash`/`immutable` 仅 footer 文本；`export_scope` 多键时**只取第一个真值键**，其余 scope 键丢；源 `updated_at` 丢 |
| `map_letta` | **type 语义丢失**（所有 archival passage 强制 `observation`）；`agent_name` 优先、`agent_id` 仅在其缺失时才入 tag；`metadata`/`source` 仅 footer；**只处理 `passages`，Letta core/recall 记忆块完全未涉及** |
| `map_supermemory` | **精确内容去重静默丢条**（`seen` 集合按 content 全等去重，重复条目直接跳过且**不计入任何 loss 报告**）；`metadata`/`score` 仅 footer |
| `map_okf` | **`expires_at` / `ttl_seconds` 在写盘路径被静默丢弃**（见下）；未知 frontmatter → footer（800/200 截断）；`links`、未映射的 okf type、原 title（>100 字符）仅 footer；**`description` 拼进 content 属内容改写** |
| `map_zep` | 见下 bi-temporal 专段 |
| `map_hindsight` | **`state=="invalidated"` 静默跳过**；`occurred_end`、`proof_count`、`entities`、`context`、`document_id`、`edited_at`、`metadata` **全部只进 footer 文本** |

**`map_zep` 的 bi-temporal 处理（最值得看的一段）**：`_zep_edge_is_current` 的规则是
**`expired_at` 非空 → 直接弃**；**`invalid_at <= now` → 弃**；`invalid_at` 为未来或 None → 保留。
也就是说 Zep 的四个时间里：
- `valid_at` → 目标 `created_at`；
- `invalid_at` → **仅 footer 文本「Valid until」，不是字段**；
- `expired_at`（系统何时标记失效）→ **只用于弃条判定，不写入任何字段**，**系统时间线整体丢失**；
- 源 `created_at`（Zep 入库时间）→ 当 `valid_at` 存在时被覆盖而丢失。

⇒ **已失效/被取代的边被直接跳过，不导入、不逐条列入报告**。runner 只把差异记成聚合的
`skipped = source_count - mapped_count`，**没有「哪些事实被丢弃及原因」的清单**。
这与铁律 6 正好冲突——对 Memanto 而言这是静默丢弃。

**另注**：`examples/migrations/9-migration-adapters/mappers.py` 是另一份简化版 `map_zep`，
**没有 bi-temporal 过滤**、不跳失效边、`type="fact"`。**两份实现不一致，示例版会把死事实复活。**

**写盘路径的二次损失（关键）**：mapper 产出的 dict 交给 CLI client 的 `batch_remember`，
但 `sdk_client.py` 与 `direct_client.py` 都**只透传 `source_ref`/`created_at`/`updated_at`**
（可选键白名单）。而目标模型 `MemoryRecord`（`memanto/app/core.py:91-143`）**根本没有
`expires_at` / `ttl_seconds` 字段**（只有语义不同的 `expired_at`/`expired_by`，指"何时被标失效"）。
⇒ **`map_okf` 唯一多保的两个字段在写盘时被静默丢弃**；`map_mem0` 的过期时间连 row 键都没落。
mapper 文档注释声称这两个字段被 `batch_remember` 接受，与实现**明确不一致**。

**共同盲区**：六个 mapper **都不处理 embedding 向量**（不读源向量、不带模型名/维度、
无重嵌入计划——对应铁律 5）；**整个 `memanto migrate` 流程无 PII/密钥扫描**。

**`x_memanto` 块的内容**：`_render_okf_doc` 写出标准 OKF 字段（`type`/`title`/`description`/
`tags`/`generated:{by,at}`/`resource`）外，私有块**固定 8 个键 + type**：
`id`、`confidence`、`provenance`、`source`、`status`、`updated_at`、`expires_at`、
`ttl_seconds`，再补 `x_memanto["type"]`；**仅当值非空才写**。

**「unmapped fields preserved, nothing is lost」在代码里怎么实现**：导入侧
`okf_loader._parse_entry` 的 `_KNOWN_FIELDS` = {type,title,description,resource,tags,
timestamp,x_memanto}，**未知 frontmatter 一律进 `extra`**，然后 `map_okf` 把 `extra`
逐键塞进 `[Supporting data]` footer。⇒ **「无损」只对固定枚举的 `x_memanto` 键成立；
其它未知字段是文本且受 800/200 字符上限截断**——是「部分保留 + 可截断」，不是真无损。
**仓库自己的适配器文档已承认**：`examples/migrations/hermes-holographic-to-okf/mapping.md`
原话 *"A large custom `x_holo` frontmatter object could therefore be truncated after
import."*；`chatgpt-claude-okf/README.md` 的 "lossless OKF round trip" 被限定为
type/source/confidence/provenance 四样存活。更进一步：**枚举内的 `expires_at`/`ttl_seconds`
也在写盘时丢**，所以「枚举集」本身也未完全往返。

**dry-run 报告的实际产物**：
- 产物 1 `mapped_preview.json`：**只是映射后 Memanto payload 的 JSON 列表**，机器可读但
  **无 schema、无源→目标 diff、无字段损失清单**。
- 产物 2 `migrate-report.md`：**只对 mem0/letta/supermemory 生成**（okf/zep/hindsight
  **无 savings report**），内容是 token/延迟/存储节省的营销口径
  （`## Your X footprint (measured)` / `## Projected impact` / `### 1. Ingestion tax` /
  `### 2. Latency & indexing` / `### 3. Storage footprint` / `## Analysis`）。
- **没有**：报告 schema、PII/密钥命中记录、审批凭据、未映射字段清单、证据等级。
  控制台摘要仅打印 source/mapped/skipped/type 计数。

**三条与我们铁律正面冲突的行为**：
1. **`--dry-run` 默认 `False`**——**默认执行真实写盘**，与铁律 2 相反。
2. **dry-run 也会调用远程模型**：只要存在 active agent，`_render_savings_report` →
   `_generate_narrative` 会调 Moorcheh `answer` 端点生成叙述，**非 opt-in**，
   调用事实只体现在报告文本里的 `llm_model`/`llm_method`，**不是结构化字段**。
3. **PII/密钥闸门仅存在于抽取示例、不在 migrate 流程**：`redact_sensitive_data`
   （正则脱敏 API key/Bearer/私钥）只服务于 `chatgpt-claude-okf` 的对话蒸馏，
   `memanto migrate` 全链路不调用。

### Remnic

- Repo: https://github.com/joshuaswarren/remnic
- Site: https://remnic.ai/
- Importers 文档: https://github.com/joshuaswarren/remnic/blob/main/docs/importers.md

MIT 开源，v9.69.x（版本号跑得极快）。`remnic import --adapter ... --dry-run`，
内容哈希幂等去重，六字段溯源。解析在本地，但 orchestrator 配远程模型做抽取时
内容照样外流。dry-run 报告只有计数、无 schema、无 PII 闸门、无审批钩子。

**8 个源适配器包**（`packages/`）：`import-chatgpt`、`import-claude`、`import-gemini`、
`import-mem0`、`import-supermemory`、`import-okf`、`import-lossless-claw`、
`import-weclone`，另有 `export-weclone`。前五个走统一
`remnic import --adapter <source>`；后两个因数据模型差异走独立命令。
这是离我们 M1 最近的实现。

配套值得读的：`docs/import-export.md`、`docs/contradiction-review.md`（对上铁律 7）、
`packages/belief-ledger`、`docs/CONVENTIONS.md`。

#### 实测发现（2026-10-05，读 importers + contradiction + belief-ledger）

**源格式真相（从 fixtures + parser 反推）**：

- **ChatGPT 新版 saved memories**：`{ "memory": [...] }`。字段 `id`（可缺）/
  `content`（**必需**，非空）/ `text`（旧别名，`content` 缺时取它）/ `created_at`
  （ISO 或 epoch）/ `updated_at`（**有则优先于 `created_at`**）/ `deleted`（true 时整条跳过）/
  `pinned`（仅 true 时记）/ `tags`（过滤非 string）。
- **ChatGPT 旧版 saved memories**：**顶层裸数组**。parser 另兼容 `{ "memories": [...] }`
  （2024/2025 shape）与 `user.memory`。旧版正文常用 `text` 而非 `content`；
  fixture 里未出现 `deleted`/`pinned`/`tags`/`updated_at`。**形状判定不看首元素**，
  而是全数组扫描首个可识别形状——避免前导 tombstone 导致整批静默丢弃。
- **ChatGPT `conversations.json`**：`Conversation{id,title,create_time,update_time,
  current_node,mapping|messages}`；`Node{id,message,parent,children}`（**parser 不读 `children`**）；
  `Message{id,author:{role,name},content:{parts},create_time,parent}`。
  取数只取 `author.role === "user"`，文本 = `content.parts` 里字符串项 join。
  活跃链从 `current_node` 沿 `node.parent ?? message.parent` 回溯后 reverse；
  **遇环或悬空 parent 则回退**到按 `create_time + node.id` 全节点排序（会包含被放弃分支）。
- **Claude `projects.json`**：`Project{uuid,name,description,prompt_template,docs,created_at,
  updated_at}` + `Doc{uuid,filename,content,created_at,updated_at}`。识别规则是
  `prompt_template` 是 string 或 `docs` 是数组。**`description` 未被 transform 使用**。
- **Claude `conversations.json`**：`Conversation{uuid,name,summary,created_at,updated_at,
  chat_messages|messages,project_uuid}` + `Message{uuid,sender,role,text,content,created_at,
  updated_at}`。取数仅 `sender ?? role` ∈ {human,user}，文本优先 `content` blocks。
  **`summary` / `project_uuid` / `message.uuid` / `message.updated_at` 均未使用**。
- **Gemini Takeout `My Activity.json`**：`header`（"Gemini Apps"/"Bard" 保留，"Search" 等过滤）/
  `title`（旧版 prompt，形如 `"Asked: <prompt>"`）/ `text`（新版 prompt）/ `titleUrl` →
  `metadata.activityUrl` / `time`（**必须 ISO-8601 UTC，非法直接抛错**）/ `products` /
  `subtitles`（name 含 "Model" → `metadata.modelTag`）/ `details`（**未使用**）。
  容器是裸数组或 `{activities|MyActivity|activity: [...]}`，无识别键则抛错。

**Remnic 的映射与「源有但它不承载」的字段**。`ImportedMemory` 的六字段溯源是
`sourceLabel`/`sourceId`/`sourceTimestamp`/`importedFromPath`/`importedAt`/`metadata`
（`importedAt` 由 `runImporter` 盖戳）。**静默衰减清单**（这就是我们迁移报告必须显式列出的字段）：

| # | 源 | 被丢弃的字段/内容 |
|---|---|---|
| L1 | ChatGPT conversation | 被放弃分支全部节点、assistant/system/tool 文本、`content_type`、`author.name`、`message.id`、`update_time`、`node.children` |
| L2 | ChatGPT conversation | 摘要超 **2000 字符**被截断 |
| L3 | ChatGPT saved memory | `created_at` 在有 `updated_at` 时被覆盖丢弃（只留一个时间戳） |
| L4 | Claude | `project.description`、`conversation.summary`、`project_uuid`、`message.uuid`、`message.updated_at`、全部 assistant 文本 |
| L5 | Claude conversation | 摘要 2000 字符截断 |
| L6 | Gemini | `details[].name`、`header`、`products`（后两者仅用于过滤）；**无 sourceId** |
| L7 | Gemini | prompt 短于 **10 字符默认静默丢弃** |
| L8 | 全部 | **任何未被 TS interface 声明的字段在 parse 阶段被白名单式丢弃**——parser 是 `normalize` 而非 passthrough，这是最大的静默衰减面 |
| L9 | 全部 | `.zip` / `.tar.gz` **直接拒绝**，要求用户先手动解压 |
| L10 | 全部 | **无 per-record 报告**：dry-run 只输出一行总数 |

**dry-run 报告形态**：只有两行
`Dry-run: would import <N> memories from '<source>'.` + `(no memories were written; ...)`。
`RunImporterResult` 只含 `memoriesPlanned/memoriesWritten/batchesProcessed/dryRun/importedAt`
——**无 schema、无 per-record 清单、无落盘 artifact、无 PII 命中记录、无审批凭据**，
dry-run 也不启动 orchestrator。已证实我们文档对它的判断。

**幂等去重的风险（推断）**：Gemini 无 `sourceId`，两条**文本完全相同**的 prompt
会被内容哈希合并成一条。

**冲突处理有两套彼此独立的机制**：

1. **contradiction-review**：nightly cron，**`enabled` 默认 `false`**；只扫
   `status === "active"` 且 category ∈ {decision, principle, rule, entity, fact, preference}。
   候选对双闸：embedding cosine ≥ 0.82 **且**（共享 `entityRef` 或 topic token Jaccard ≥ 0.4）。
   判官是 LLM-as-judge，verdict ∈ `contradicts|independent|duplicates|needs-user`，
   **任何失败默认 `needs-user`**，内容 hash 缓存避免重复调用。
   **复合键**：`pairId = sha256(("ns:<namespace>::")? + sorted(idA,idB).join("::")).slice(0,24)`
   ——（有序记忆对 + 可选 namespace）的确定性哈希，保证重跑幂等；落
   `memoryDir/.review/contradictions/<pairId>.json`。
   呈现三面：CLI（`openclaw engram review list|show|resolve`）、HTTP、MCP（`review_list`/
   `review_resolve`/`contradiction_scan_run`）。
   裁决动词：`keep-a`/`keep-b`/`merge`/`both-valid`/`needs-more-context`；
   `autoMergeDuplicates` 默认 `false`，**verdict=`duplicates` 也只是 flag，仍需人批**。
   写回：`executeResolution` 对 keep-a/keep-b 调 `storage.supersedeMemory(loser, winner)`，
   **真改 frontmatter**：`status:"superseded"` + `supersededBy` + `supersededAt`；
   `merge` 新建 merged memory 后 supersede 两条。
2. **belief-ledger**：`LedgerClaim = {id, memoryId, statement, kind: claim|prediction|opinion,
   stance: for|against|uncertain|neutral, confidence, scope:{entities,domain,timeWindow},
   deadline?, evidenceLinks[], status: active|superseded|resolved|snoozed|ignored, …,
   resolution?:{verdict, actualConfidence, brierScore?}}`。状态机
   `active → snooze|ignore|supersede|resolve`，`split` 建 N 条带 `parentIds` 并 supersede 原条。
   落盘成 Remnic `fact` 记忆，`tags` 带 `belief-ledger:*` 前缀，`structuredAttributes` 打
   `ledger.*` 前缀。发现流程是 `capture → crossExamine`，`scoreCandidate` 加权打分
   （**必须至少一个 topical match，否则 score=0**），逐对 LLM 判
   `contradiction|evolution|refinement|unrelated`；**只有 `contradiction` 才生成
   `LedgerChallenge{question, priorClaimIds, suggestedActions}`**——即"苏格拉底式提问让人回答"，
   而不是列表页。还有过期 prediction 按 deadline 打 Brier score 的 calibration 闭环。

**「裁决结果写回复合键」在 Remnic 里的确切形式**：真正执行 supersede 用的是
`${normalize(entityRef)}::${normalize(attributeName)}`（normalize = trim→lowercase→
空白/连字符归一为 `-`），`supersessionKeysForFact` 为每个属性各产一个键。

**做到哪一步 / 没做到哪一步**：

| 环节 | 做到了 | 未做到 |
|---|---|---|
| 候选聚类 | embedding + entity/topic 双闸；内容 hash 缓存；确定性 pairId | 只 pair-wise，无多记忆群体冲突 |
| 判定 | LLM-as-judge 四分类；失败默认 `needs-user`（保守） | 唯一判定器是 LLM；scan 默认关闭且只扫 6 类 category |
| 呈现 | CLI/HTTP/MCP 三面 + Socratic 提问 | 无"冲突清单"式全量导出 |
| 人裁决 | 5+3 个 verb；冷却 14d/24h | 无自动合并 |
| 写回 | 真改 memory 状态，复合键 = `entityRef::attrName` | 无记忆版本链/下游传播的撤回闭环 |

**结论**：Remnic 在铁律 7 上比我们多走了"裁决执行器"这一段，**但触发裁决的仍然是人**；
它没有把「源有但目标不承载」显式化（铁律 6）。

**该学**：dry-run 在管线层截断（`dryRun` 时直接 return，永不调 `writeTo`，无法被适配器绕过）；
三方法契约 `parse → transform → writeTo` 且 `writeTo` 用窄接口便于测试替身；
确定性输出（会话链遇环即回退排序、pairId 用 sorted ids 的 sha256）；
形状探测健壮（全数组扫描、`undefined/null` 输入永远抛错绝不"0 条成功"）；
每个 adapter 独立 package + optional peer dep + 动态 import 友好提示；
fixture 全部合成数据无真实 PII；保守默认（judge 失败 → `needs-user`、scan 默认关、
autoMerge 默认关）；裁决后真的执行写回 + 复合键；cooldown + 内容 hash 失效重判。

**明确不学**：dry-run 报告只有计数（违反铁律 2）；**无 PII/secret 出站闸门**
（importers 全链路无脱敏调用，它自己的文档承认配了远程抽取模型时内容照常外流）；
**无审批钩子**（写盘不需人确认）；静默衰减（白名单丢弃、2000 字截断、`created_at` 被覆盖、
Gemini <10 字符静默丢）；把 LLM 作为唯一矛盾判定器且用 host 配置模型；
不支持 ZIP（真实迁移场景 ChatGPT/Claude 导出都是 ZIP）；不带 embedding 模型名+维度、
无结构化损失计划；冲突扫描默认关闭（会让人误以为"没有冲突"）。

**一条待我们实测确认的负面样本（推断）**：`ImportTurn` 的六字段 provenance 只在 **turn 层**，
最终 memory frontmatter 是否保真未被证实；若不保真，则 Remnic 的"六字段溯源"落盘后可能已缩水
——这正是我们迁移报告要显式声明的那类衰减。

### Supermemory

- Repo: https://github.com/supermemoryai/supermemory
- 迁移文档: https://supermemory.ai/docs/migration/from-mem0

本地可跑的 Memory API + app。`apps/docs/migration/` 下有 `from-mem0.mdx`、
`mem0-migration-script.py`、`from-zep.mdx`。文档把迁移写成完整工程流程：
把 mem0 的 users / agents / apps / runs 映射到 container tags、回填记忆、
验证检索、带回滚路径地切换。**唯一把迁移方法论写成文档的商业玩家。**

### Zep / Graphiti — 双时序知识图谱记忆

- Graphiti Repo: https://github.com/getzep/graphiti
- Zep Cloud: https://www.getzep.com/
- Zep 仓库（现为 Cloud 示例库）: https://github.com/getzep/zep
- CE 废弃公告: https://blog.getzep.com/announcing-a-new-direction-for-zeps-open-source-strategy/

Zep Cloud 是托管记忆平台（Step 3 实验对象）；其开源本体是 Graphiti（MIT，进程内库，
Neo4j/FalkorDB/Neptune 后端），做实体/边抽取、bi-temporal 时间语义（valid_at /
invalid_at 分立）与边失效。**Zep Community Edition 已废弃**：代码移入 zep 仓库
legacy/ 目录，zep 仓库本身改为 Cloud 的 examples/integrations。废弃前的 CE 架构
= zep API + pgvector + graphiti graph service + Neo4j。记忆平台 OSS 版说死就死的
活样本（2026-09 核实）。注意 Memanto 与 Remnic 都把 zep 当迁移源，
且 Remnic 有 `docs/graph-edge-decay.md` 处理边衰减。

## 学术参照

### PAM — Portable Agent Memory

- 论文: https://arxiv.org/abs/2605.11032
- 实现: https://github.com/santhoshravindran7/portable-agent-memory

跨异构 LLM agent 的可验证记忆转移协议：Merkle-DAG 溯源 + 抗注入多级再水化管线
（structural framing / content escaping / type enforcement），实验样本 N=50。
撤回闭环方向的学术坐标。UMP §5.3 的 injection-resistant rehydration 是同思路。

## 观察位（暂不投入）

- **MemOS**（https://github.com/MemTensor/MemOS）：提出 MemCube 统一记忆抽象
  （parametric / activation / plaintext 三类），`mem_cube/` 有 dump/load。
  但这是引擎内部的调度表示，不是对外交换格式，无跨厂商采纳证据。
- **akitaonrails/ai-memory**（https://github.com/akitaonrails/ai-memory）：
  定位就是「给 agent CLI 做长期记忆 + 方便跨 agent 厂商交接」，Rust 实现。
  `docs/` 是一整套竞品调研（research-agentmemory / basic-memory / cognee /
  hindsight / codebase-memory-mcp / ecc / karpathy-llm-wiki、
  `research-2026-landscape.md`、`competitive-parity.md`、`comparison.md`、
  `issues-*.md`、`okf.md`、`wiki-migrations.md`）——**与我们的文档直接重叠，
  值得对读而不是照抄**。顺带带出新名字 `mempalace`、`ecc`。
- **doobidoo/mcp-memory-service**（https://github.com/doobidoo/mcp-memory-service）：
  有 importer 与 mem0 转换器 tracking issue。是记忆服务顺手加的导入功能，
  不是迁移工具。看一眼粒度即可。
- **memcommons/spec**：见上，是寻址/权限语义而非格式，不作映射目标。

## 未跟踪项目名（待分档）

来源：OMPI 的 `prior-art/` 六份文档（interoperability-protocols / emerging-standards /
academic-foundations / agent-harness-memory-systems / enterprise-memory-services）。
**加粗 = 我们此前完全没跟踪**。这里只列名字与一句话定位 + 我的分档判断，不深挖。

### 传输与信任层（多数不调研，但有例外）

- **PTC**（Provenance and Trust Context，LF Edge + AAIF，v0.2.3-draft）——跨边界信任层：
  签名信任上下文 + append-only 溯源链 + Biba 完整性格 + 无模型确定性闸门。
  **值得单查一次**：它定义的正是"溯源 + 信任"凭证形状，如果我们迁移报告的凭证要跨组织验证，
  应该复用而不是自创。
- **GAL**（Grant and Autonomy Lifecycle，PTC 配套）——自治阶梯。不调研。
- **ANP**（Agent Network Protocol，v1.1，IETF Internet-Draft）——DID 身份 + JSON-LD
  能力描述，领 W3C AI Agent Protocol CG。不调研（身份层，非记忆格式）。
- **AGNTCY**（Cisco / Linux Foundation，49 repos）——agent 发现/身份/消息/可观测；
  含 **OASF**（Open Agent Schema Framework）、**SLIM**（量子安全 pub/sub）；
  其 **ACP**（源自 IBM）2025-08 并入 A2A 后归档。不调研。
- **AG-UI**（CopilotKit）——agent→前端事件流，"第三支柱"。不调研。
- **Agent Plugins**（agent-plugins.org/specification）——agent 能力分发插件规范。不调研。
- **DTI**（Data Transfer Initiative，Apple/Google/Meta 资助）——消费者数据可移植非营利。
  **值得看治理模型**：它是"用户数据跨平台可移植"这件事最成熟的治理先例。
- 加密/身份原语：Ed25519、DSSE、in-toto、Biba lattice。用到再查。

### 标准组织动态（判断"该跟谁对齐"时有用）

- **AAIF**（Agentic AI Foundation，Linux Foundation，约 290 成员）——治理 MCP / A2A /
  Goose / AGENTS.md，**但没有记忆专门工作组**。这条本身就说明记忆标准化的空位。
- **W3C**：AI Agent Protocol CG（2025-05，约 180 人）；
  **AI Agent Memory Interoperability CG（2026-06 成立，宪章 v1.0）**——
  **这是唯一一个专门做"记忆互操作"的标准工作组，值得跟踪**。
- **IETF**：`agentproto` BoF（IETF 126，2026-07）、`draft-rosenberg-ai-protocols`、
  `draft-daniel-ai-agent-internet-architecture`；**SAIHM**
  （`draft-saihm-memory-protocol-01`，**唯一专门讲 agent memory 的 IETF 草案**，
  但已出队未推进）。
- **NIST**：AI Agent Standards Initiative（2026-02）、NCCoE（六身份标准：OAuth2/2.1、
  OIDC、SPIFFE/SPIRE、SCIM、NGAC、MCP）、AI Agent Interoperability Profile（计划 2026-Q4）。
- **OASIS / CoSAI**（Coalition for Secure AI，Anthropic + IBM 共同主持）——
  2026-04 发布 Agentic Identity and Access Management。**合规字段可能要引用它**。
- **CNCF**——2025-06 提出 "AgentMemory API" 缺口。观察。

### 格式与提案（这里有真该看的）

- **Context Nest**（PromptOwl，arXiv 2607.02116）——八个 frontmatter 键 + 散文链接、
  `contextnest://` 寻址、哈希链版本 + checkpoint、**forget 协议**。
  **值得看 forget 协议与哈希链版本**：直接对上我们的删除语义与版本链。
- **EngramSpec**（Ashish Verma / **8mem**，engramspec.org）——HTTP 可移植记忆，
  **IDENTITY / BELIEFS / CORRECTIONS / EVOLUTION 四对象**。
  **值得看**：`CORRECTIONS` + `EVOLUTION` 两个对象直接对上铁律 7（冲突/演化不裁决）。
  附带新名 **8mem**、**PLUR / plur.ai**。
- **Memoryfield**（2026-09）——zip（Markdown + SQLite 向量）简易格式。
  **值得看一眼形状**：这是"Markdown + 向量同包"的一种 bundle 切法。
- **MMP**（Mesh Memory Protocol，arXiv 2026-04，v0.2.3）——八层多 agent 记忆架构。观察。
- **MEMORY.md**（Cromus.ai，v0.2.0）——agent 写持久 markdown 文件的规范。观察。
- **ATIF**（Agent Trajectory Interchange Format，Harbor Framework）——轨迹日志格式。观察。
- **Letta Trajectory Library**（Letta）——15 种 harness 轨迹归一库，
  **四级身份派生：native / location / content / synthetic**。
  **值得看身份派生那四级**：直接对上我们五问表第 2 问（身份字段跨系统对不上）这个重灾区。
- **AMS Card**（Mila + Mozilla）——记忆系统的结构化**文档**格式。
  **值得看一眼**：可以当迁移报告的自我描述层。注意字段结构不在 OMPI 仓库里，
  要去 `github.com/mila-iqia/agent-memory-system-card`。
- **me.md**（David Hamilton / Block·goose）——用户自有纯文件，agent 只能**提议**，
  用户决定。**这个立场和我们的审批钩子同构**，值得一看。
- **SAIHM**（Russell Jackson）——加密记忆单元 + 后量子身份。观察。
- **COGX**（Cognee）——已在 ecosystem 跟踪（cognee）。

### 企业记忆服务（潜在 Writer 目标）

- **AWS AgentCore Memory**、**Google Vertex Memory Bank**、**Microsoft Foundry Agent
  Service Memory**——三家云厂商的托管记忆。**要新增目标平台时优先看这三个**
  （体量最大、最可能是用户实际在用的）。
- **ChatGPT Memory**（产品侧记忆，与"ChatGPT 导出"不同）——观察。
- **Pi**（Inflection）——极简/黑箱形态的代表。观察。

### harness（源侧 Reader 的潜在对象）

- 已跟踪：Claude Code、Codex、Gemini CLI、OpenClaw、Cursor。
- **未跟踪**：**Cline / Roo Code**、**Continue**、**Aider**、**OpenHands**、
  **GitHub Copilot**、**Hermes Agent**（Nous Research）、**Pi**。
  **只登记，不做 Reader**：这些是"要支持新源"时的候选池，现在不需要逐个看落盘形态。

### 学术参照（只登记）

**CoALA**（TMLR 2024）、**Generative Agents**（UIST 2023）、**MemGPT**（→ Letta）、
**LongMemEval**（ICLR 2025）、**MemoryAgentBench**（ICLR 2026）、**AgentMemBench**（2026-06）、
**Memory-R1**（2025-08）、**MemoryBank**、**Portability study**（arXiv:2609.05339）。

**唯一值得读的一篇**：**MINJA**（记忆注入攻击）——它研究的是"往记忆系统里注入恶意记忆"，
直接关系我们 Reader 面对不可信源内容时的处理（与 UMP §5.3、PAM 的抗注入再水化同一问题域）。
其余是评测基准与架构论文，用到再查。

## 已排除（附理由）

- **ai-akashic/open-memory-protocol**：只是名字撞车（"Open Memory Protocol" 不唯一）。
  独立项目的 `1.0-draft`，目录 + JSONL 包 + REST 运行时。版本号、schema、
  协议形状都不能与 OMPI 混用。
- **UHP — Unified Harness Protocol**：产品↔服务器↔完整 agent harness 的执行契约，
  不定义持久记忆的文件布局。与记忆格式正交，可组合但不互相依赖。
- **MemoryPlugin / MemoryLake / MemX**：浏览器扩展、内容营销、消费级笔记 app。
  只提供「怎么点按钮」的指南，无新机制。
- **MemU / cognee / Hindsight / LangMem / OpenClaw 插件群（MemClaw、lossless-claw、
  qmd）**：记忆引擎，不是迁移工具。只在「要新增目标平台」时才需要逐个看落盘形态。
  cognee 另在 `early-draft-specs/` 里对 OMPI 提了一份草案，说明它在意互操作。
