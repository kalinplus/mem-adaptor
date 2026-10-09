# 设计：我们怎么做，为什么

[AGENTS.md](../AGENTS.md) 里的铁律是**结论**。这份文档解释结论是怎么来的：
每条决策给出「证据 → 理由 → 后果」，证据来自本仓库的实测（见
[step4-5-report.md](step4-5-report.md)、[memory-products.md](memory-products.md)、
[source-memory-formats.md](source-memory-formats.md)）和 2026-10-05 对 15 个外部仓库的
调研（见 [ecosystem.md](ecosystem.md)）。

改动设计时先读这里；只改实现不必读。决策编号用 `DEC-n`，与 AGENTS.md 里 MVP 节奏的
「D1 / D2–3」（交付阶段）区分开。还没拍板的问题集中在 [§6](#6-待定问题)。

## 0. 一句话

我们是**搬家的施工队**：把记忆从源系统读出、归一、清洗、去重，按目标系统形状写入，
每一步产出一份机器可验证的凭证。不自建交换格式，不自建中心存储，不自建治理机制，不做冲突裁决。

两种用法：**直迁**（源 → 目标，一次性，例如 ChatGPT → Claude）和**家模式**（用户在现有格式里
选一个「家」，其余工具都是卫星，我们定期把卫星汇总进家并对账，见 DEC-19）。
家模式让工具持续有用，又不需要我们发明存储格式。

### 默认策略：增量汇总，不是镜像同步

2026-10-07 用户确认：这条策略适用于所有来源、目标和实现阶段，后续需要调整再另行确认。
只新增、按已批准规则更新本轮确实存在的记录；来源缺席不等于用户要求删除。

- 能完整读取但零条记录时，可产出无新写入的计划/回执，目标保持原样；旧回执的自身、重复代表及共享依据仍保留。
- 某条源记忆本轮缺席时记为 `source_missing`，不声称有意删除，也不将旧验证算成本轮新验证。
- 源路径不存在、读取/解析失败或历史依据失效时停止，不将故障伪装成空源。
- 明确删除意图继续待人工裁决。镜像同步、删除传播及保留式/移除式删除的目标映射都需另行确认，不属于当前默认行为。

增量汇总不取消目标编辑保护、防复活、逐条拒绝或审批。审批必须绑定本轮全部执行依据，
包括空源轮次仍要携带的旧历史；其他来源与用户自己添加的内容不能因源为空被清理。

## 1. 架构与数据流

```
核心引擎：解压 → 源识别（已注册的 Reader 认领文件）
  → Reader 插件（源适配器，社区可贡献）：解析 → 归一，交出源记录 + canonical 记录
  → 核心引擎（全部本地执行）：校验 canonical → 密钥/PII 闸门 → 去重/冲突聚类
      → dry-run 闸门（产出计划报告）→ 人工确认/审批钩子
  → Writer 插件（目标映射器，可多目标同时导出）
      → 写入 → 回读验证（产出回执报告）

横切：治理接口（approval receipt / 快照 / 审计链 / 导出授权）
      conformance 套件、CLI 优先 MCP 为辅
```

九个阶段，各自的负责方和可验证产物：

| # | 阶段 | 负责方 | 输入 → 输出 | 产物 |
|---|---|---|---|---|
| 1 | 解压与源识别 | 引擎解压；已注册的 Reader 逐个声明「这些文件归我」 | ZIP/目录 → 文件清单 + 源类型判定 | 源清单（**含没有 Reader 认领的文件**） |
| 2 | 解析 | Reader | 源文件 → 源记录（**原样字段，不做归一**） | 源记录流 |
| 3 | 归一 | Reader 产出，引擎校验（DEC-17） | 源记录 → canonical record | canonical 记录 + 字段映射表 + 未承载字段清单 |
| 4 | **密钥/PII 闸门** | 引擎 | canonical 记录 → 检测 → 按用户策略放行/拦截，或替换为引用 | 命中清单（规则 id + 位置 + 处置，**不含命中值**） |
| 5 | 去重与冲突聚类 | 引擎 | → 去重后记录 + 冲突候选簇 | 冲突清单（确定性簇 id） |
| 6 | dry-run 闸门 | 引擎 | → 计划报告（**不写目标**） | **计划报告**（核心资产） |
| 7 | 人工确认/审批 | 治理后端 | 计划报告 → 审批凭证 | approval receipt（绑定计划摘要） |
| 8 | 写入 | Writer | canonical 记录 → 目标形状 → 写入 | 逐条目标 id |
| 9 | 回读验证 | Writer（用目标自己的读接口） | 目标 → 比对结果 | **回执报告** |

硬约束：**第 4 阶段先于一切出站路径**（DEC-1）；**第 6 阶段的截断点在引擎层**（DEC-11）；
**第 8 阶段写入前核对审批凭证绑定的计划摘要**（DEC-3）。

## 2. canonical model（内部脚手架）

canonical model 是**内部数据结构**，不是对外格式（铁律 1）。它唯一的存在理由是
让 n 个 Reader × m 个 Writer 变成 n + m 份适配器，而不是 n×m 份配对转换。

字段分组，以及每组为什么存在：

**身份层**（跨系统丢信息最严重的地方）

| 字段 | 说明 |
|---|---|
| `canonical_id` | 本仓库生成的稳定 id |
| `source.system` / `source.adapter_version` / `source.export_version` | 哪个源的哪个版本；**导出格式版本必须记**，因为 ChatGPT/Claude 都有新旧两版 |
| `source_record_id` | 源系统原始 id，**原样保留不解析**（各系统 id 形状互不兼容） |
| `source_locator` | 在导出包里的路径（ZIP 内路径），用于回溯 |
| `scope` | 从源读到的地址：`user`/`project`/`agent`/`session`/`tenant`/`wing`/`room`——**是地址，不是权限**（DEC-13） |
| `scope_qualifier` | 独立字段，回答「这是哪个项目/哪个用户」（DEC-14） |
| `owner_declared` | 源系统自声明的 owner，**标记为不可信**（铁律 4）；不进 `scope` |

**内容层**

| 字段 | 说明 |
|---|---|
| `content` | 归一后的正文 |
| `content_hash` | `sha256(content)`；完整性校验与「内容变没变」的判断依据（**不是**身份，见 DEC-15） |
| `source_kind` | 源系统的 kind 原样值（不翻译，翻译在 Writer 做） |
| `source_extra` | M4 经用户确认：可选的逐条未知源 metadata，不装全文、导出文件或 canonical 快照；参与完整记录哈希与密钥检测，报告只列路径与保留位置 |
| `dna_class` | `dna` / `standard`（DEC-4） |
| `tags` / `entities` / `relations` | 各源形态差异极大，统一为「有类型的边」 |

**时间层**（双时序）

| 字段 | 说明 |
|---|---|
| `created_at` / `updated_at` | 记录时间（何时入库/被改） |
| `observed_at` / `valid_from` / `valid_to` | 事实时间（这件事何时为真）；与记录时间成对出现（DEC-14） |
| `expires_at` / `ttl` | 过期；Memanto 目标侧不承载（DEC-4），mem0 OSS 承载 `expiration_date` |

**溯源层**

| 字段 | 说明 |
|---|---|
| `provenance.actor` / `actor_kind` | `user` / `agent` / `model` / `import` / `scan` |
| `provenance.method` | 怎么产生的（`import` / `llm_extract` / `filesystem` …） |
| `provenance.source_ref` / `evidence[]` | 原始出处 |
| `evidence_level` | `实测` / `官方文档` / `第三方核对` / `推断`——**报告里要区分**，不能让用户以为推断是实测 |

**治理层**

| 字段 | 说明 |
|---|---|
| `consent.exportable` / `retention` / `redact[]` | 能不能出站、留多久、哪些路径要剥 |
| `consent.memory_enabled` | 源侧该 scope 是否开着记忆（DEC-14） |
| `approval.state` / `receipt_ref` | 审批状态与凭证引用（凭证本体不内联） |
| `sensitive_findings[]` | 闸门命中清单：密钥 / 高危 PII / 一般个人事实三级（DEC-1） |
| `deletion_intent` / `tombstone` | 用户的删除意图；墓碑记录（DEC-2、DEC-14） |

**向量层**

| 字段 | 说明 |
|---|---|
| `embedding.vector` / `model` / `dim` / `normalized` | **模型名+维度必须随记录走**（DEC-5） |
| `reembed_plan` | 目标不支持该模型时的结构化计划 |

**冲突层**

| 字段 | 说明 |
|---|---|
| `conflict_cluster_id` | 确定性簇 id（DEC-6） |
| `conflict_candidates[]` | 候选清单，只聚类不裁决 |
| `verdict` | 人的裁决结果（写回，供重跑时不再重复询问） |

## 3. 核心资产：迁移报告 schema

报告是本仓库**唯一对外承诺 schema 的东西**，同时是收据、合规凭据、用户可读解释。
对照现有工具的 dry-run 产出：

| 工具 | 它的 dry-run 产出 | 缺什么 |
|---|---|---|
| Remnic | 一行总数：`Dry-run: would import N memories from 'X'.` | schema、逐条清单、损失清单、PII 命中、审批凭据 |
| Memanto | `mapped_preview.json`（映射后 payload 列表）+ 营销口径的 savings report（**只对 mem0/letta/supermemory**，okf/zep/hindsight 没有） | schema、源→目标 diff、字段损失清单、证据等级 |

**报告分两份**：

| 报告 | 何时产出 | 逐条状态的含义 |
|---|---|---|
| 计划报告 | 第 6 阶段（dry-run） | **预测**：按 Writer 的能力声明推算每条会怎样 |
| 回执报告 | 第 9 阶段（写入并回读后） | **实际**：目标真存下了什么 |

分两份的原因见 DEC-4 的 Memanto 教训：映射层认为保留了的字段，写盘时可能被目标静默丢掉。
只有计划报告，就只能证明「我们打算怎么做」，证明不了「做成了什么」。

**逐条处置取 OMPI 的五态**（`prior-art/convergence-analysis.md`）：

| 态 | 含义 | 来自哪个阶段 |
|---|---|---|
| `accepted` | 原样承载 | Writer |
| `transformed` | 承载但被改写（**必须写出改写内容**） | Writer |
| `omitted` | 不写入，显式丢弃（**必须写出原因**：目标不支持 / 精确重复 / 裁决落选） | Writer、去重、裁决 |
| `unresolved` | 需要人裁决才能决定（冲突未裁决、DNA 类无法承载） | 去重、Writer |
| `rejected` | 违反闸门或约束，拒绝写入 | 闸门（策略设为 `block` 时） |

闸门命中但按策略放行（或被逐条显式放行）的记录是 `accepted`，命中与处置照样记入 `sensitive_findings`。

回执报告在处置之外再加一个**验证结果**：`verified`（回读一致）/ `mismatch`（回读不一致，
附差异）/ `unverifiable`（目标无法回读，如网页粘贴，见 DEC-15）。

每条回执带：源 id → 目标 id 的**身份映射**、命中的规则、原因、证据等级。

报告顶层的固定字段：源系统 + 导出格式版本、canonical model 版本、adapter 版本、
**目标能力声明**（目标支持什么/不支持什么）、**模型调用事实**（我们 opt-in 的远程调用 +
目标侧会触发的调用，DEC-12）、**闸门策略及其来源**（用户选择 / 默认值，DEC-1）、
审批凭证引用、**计划摘要**（DEC-3）、**bundle manifest**（DEC-14）。

**报告本身是敏感文件**：它为了可读会带内容预览，等同于记忆数据本身对待。
闸门命中的值不得出现在报告里（DEC-1）。

## 4. 关键设计决策

### DEC-1. 密钥/PII 闸门在引擎里，先于一切出站路径；检测常开，处置默认放行、由用户选

**证据**：portable-memory 的 `secretRef` schema 描述原文是 *"a reference to a vault
secret. Carries encryption metadata only; NEVER the plaintext or ciphertext"*，`preview`
字段明写 "MUST NOT contain recoverable secret material"；UMP 的对应机制 `consent.redact`
是 JSON-path，作用于**导出时剥离**。**两个格式都没有"内联脱敏值"这个位置。**
另外八套实测系统的 PII 拦截**四连零**——源和目标都不设卡、也不提示，要让用户知道有什么要出去，只能靠我们。

同一个事实也说明**放行是用户已经习惯的行为**：在任何一家记忆工具里存进去的密钥都原样留着。
我们默认拦截，用户会在汇总了几轮之后才发现某些记忆一直没过来，这比放行更让人意外。

**理由**：出站路径有四条——Writer 产物、报告正文、opt-in 的远程抽取、远程 embedding
（去重或目标侧）。用户要能在任何一条路径之前知道「哪些东西要出去了」，所以检测必须先于这四条。
检测放在引擎而不是 Reader 里，理由与 DEC-11 相同：**由插件负责的检查，就是可以被绕过的检查。**

**检测和处置分开**：检测永远开着、不可关闭（全本地、成本低）；处置是用户的策略，默认放行。
我们的价值在「告知」而不在「代用户做决定」：别家既不拦也不说，我们至少说清楚。

**后果**：

- **三级检测，每级一个处置策略**（`pass` 放行 / `block` 拦截），默认值如下。记忆本来就是关于用户本人的，
  「我住在北京」是 PII，也是这条记忆的全部意义，所以一般个人事实只列出、不提供拦截。

  | 级别 | 例子 | 默认 | 设为 `block` 时 |
  |---|---|---|---|
  | 密钥 | API key、token、私钥、密码 | `pass` | `rejected`，不提供逐条放行；误报靠规则级白名单，白名单本身记入报告 |
  | 高危 PII | 证件号、银行卡号、手机号 | `pass` | `rejected`，**可逐条显式放行**（放行后 `accepted`，放行动作记入报告） |
  | 一般个人事实 | 住址城市、职业、偏好 | 只列出 | — |

- **放行不是静默放行**：命中一律列入 `sensitive_findings`；每次运行 CLI 都打一行摘要
  （如「检测到 3 条疑似密钥、1 条高危 PII，按当前策略放行」），计划报告顶部同样醒目列出。
- **策略在用户第一次用的时候就选**，不能让用户用了一半才发现规则：
  家模式由 `mem-adaptor init` 询问，写进家目录的 `.mem-adaptor/config.toml`（DEC-19）；
  直迁没有初始化步骤，第一次运行时询问，写进用户级配置。询问时默认选项是放行，并说明放行的后果
  （密钥会原样进入目标；远程目标发出去就收不回）。
  非交互运行（脚本、CI）且没有配置时按默认放行，并在输出和报告里标明「策略来自默认值，未经用户选择」。
- **策略进报告、进计划摘要**：报告顶层记生效策略及其来源（用户选择 / 默认值）；策略纳入计划摘要（DEC-3），
  审批批准的是「在这个策略下的这份计划」，事后改策略需要重新出计划报告。
- **一个策略管所有去向**：本地文件目标、远程目标、opt-in 的远程模型调用不分开设。
  远程去向发出去就收不回，风险更高，但 MVP D1 只有本地文件目标，等 D2–3 接 Mem0 Writer 时再议（§6 Q9）。
- **源里本来就是密钥条目的不受策略影响**：如 portable-memory 的 `secretRef`，源里本来就没有明文，
  只能**替换为引用**——记「这里有个密钥、值未随行」，Writer 映射成目标的对应物（portable-memory 是 `secretRef`，
  其他目标 `omitted` 并写原因）。
- 报告只记规则 id + 位置 + 处置，**不复述命中值**；这一条与策略无关，放行模式下也成立，
  否则报告本身就成了第二个泄露点。
- 交付节奏：**MVP D1 就做密钥的正则检测与策略选择**——D1 产出的 UMP/OKF 文件和报告正是用户会拿去分享的东西；
  PII（接现成 NER）放在 D2–3。
- `lab/` 全程未测检测，这是待补的最大验证缺口。

### DEC-2. 删除语义按目标分叉，不共用一套

**证据**：两个格式的删除语义**相反**。UMP `forget` 默认只把 `lifecycle.status` 改
`tombstoned`、**整条记录保留**（内容还在），`hard:true` 也只是把 `body.text` 换成
`[erased: reason]`（`src/server.ts:274-294`）。portable-memory 的 `delete` 是**物理移除内容
及全部派生物 + 永久留墓碑 + proof-of-reach**，导入顺序强制"**先应用 tombstones 再 merge**"，
且有 `No resurrection` 规则覆盖每一种 kind。

**理由**：删除语义不是格式细节，是法律语义（GDPR 第 17 条「被遗忘权」）。强行统一会在其中一侧说谎。

**后果**：canonical model 只记「用户意图 + 墓碑」，Writer 分别映射。
报告里必须写明"本次迁移到 A 是墓碑式、到 B 是保留式"，因为同一份记忆在两个目标上
的"已删除"含义不同。具体映射规则还没设计（§6）；防复活靠上一次的回执报告（DEC-18）。

### DEC-3. 迁移报告是一等 artifact，审批绑定计划摘要

**理由**：所有现有工具的报告都是"打印给人看"的，没有一个能机器验证。
而迁移这件事的信任成本恰恰在于"我凭什么相信你说迁成功了"——需要可复核的逐条回执（第 3 节）。

**后果**：

- 报告分计划与回执两份（第 3 节）。
- 审批凭证绑定**计划摘要**（canonical 记录集合的哈希 + 目标 + Writer 版本 + 闸门策略）。
  Writer 写入前重算，不一致就拒写。否则用户批的是计划 A，实际写进去的是重新跑出来的 B。
- M3 经确认补充 `record_hash = sha256(JCS(canonical record))`，不只绑定正文；
  逐目标处置预测中的 `prior_write` 也参与摘要，历史依据包含目标 id、正文哈希、
  canonical 元数据哈希与验证结果。正文不变但 scope/consent 变动也须重新审批。
- M5 经确认增加原生 `target_hash`、目标文件/共享产物 `artifacts` 快照和 `target_map`；
  展示标题、目标创建时间及索引/日志/数组的变更也受到审批与历史保护。
  WriteToken 核对 Writer 实际读取的字节，不仅依赖 canonical 回读哈希。
- M0–M5 review 后修复：自身 `prior_write` 与重复代表 `duplicate_write` 分离；
  Writer 对实际输出字节出具证明，回执核对后采纳，不以重读观察值代替写入依据。
  JCS 入口拒绝超出安全范围的整数，密钥检测同时扫描对象键名。

### DEC-4. DNA 等级 + 未承载字段必须显式

**证据**：三份实证。① **Remnic 有 10 条静默衰减**，最狠的是 parser 是白名单 `normalize`
而非 passthrough——**任何未在 TS interface 声明的字段在解析阶段就丢**；另有摘要 2000
字符截断、`created_at` 被 `updated_at` 覆盖、Gemini prompt 短于 10 字符静默丢。
② **Memanto** 把无法入 schema 的字段塞进 `[Supporting data]` footer，**总长上限 800 字符、
单值 200 字符**，超长截断；它的适配器文档自己承认 *"A large custom `x_holo` frontmatter
object could therefore be truncated after import."* ③ **AIMEM**（AI Memory，IETF 个人草案）
定义了 DNA 类（`preference`/`decision`/`identity`/`pitfall`/`procedure`）**不得静默衰减、删除或取代**。

**理由**：静默丢字段是记忆迁移最隐蔽的伤害——用户以为迁过去了，实际少了最关键的那条。

**后果**：canonical model 给每条记忆标 `dna_class`。Writer 无法承载时：
DNA 类 → `unresolved`，**阻断写入直到人确认**；standard 类 → `omitted` 并写原因。
Reader 同样要交出未承载字段清单（第 3 阶段产物），不能学 Remnic 在解析阶段白名单丢字段。
**Memanto 的一个具体教训**：它的 `map_mem0` 算出 `expires_at` 却没落 row 键，
`map_okf` 落键了但目标模型 `MemoryRecord` 根本没这个字段，写盘时被静默丢——
**"映射层保留了"不等于"目标真存下了"**，这是回执报告和第 9 阶段回读验证存在的原因。

### DEC-5. embedding 模型名+维度必须随记录走，禁止静默重嵌入

**证据**：实测八套系统，**没有一家把 embedding 模型名写进记录**（Memoria 只把维度
`VECF32(2048)` 写进 schema）；Memanto 六个 mapper **全都不处理向量**（不读源向量、
不带模型名/维度、无重嵌入计划）。唯一成文规定的是 AIMEM：**envelope 必须声明
`embedding_dim` + `embedding_model`**，且 Consumer 不支持该模型时 MAY 丢弃 embedding
但**必须产出结构化警告**，**MUST NOT 静默重嵌入**。

**理由**：向量离开产生它的模型就没有意义。静默重嵌入会让检索质量悄悄变化，
且用户无法察觉——这是"静默衰减"里最难发现的一种。

**后果**：`embedding` 必须带 `model` + `dim`。目标不支持时输出 `reembed_plan`
（哪些条、用什么模型、质量影响预估）。注意网页端导出（ChatGPT/Claude/Gemini）本来就
**没有向量**，这一层只对数据库式源（mem0、Memoria 等）有值；对网页端源，重嵌入是目标侧
必然发生的事，计划报告照样要写明「目标将用模型 X 重新嵌入 N 条」。

### DEC-6. 冲突只聚类不裁决，簇 id 确定，裁决结果写回

**证据**：Remnic 做到了"裁决执行器"这一段，值得学的两点：① **`pairId` 的确定性**——
`sha256(sorted(idA,idB))` 保证重跑幂等、不会重复骚扰用户；② **裁决回写复合键**——
`${normalize(entityRef)}::${normalize(attributeName)}`，写入时把同 `entityRef + attribute`
的旧事实标 `superseded`。但它的**触发者仍然是人**（`autoMergeDuplicates` 默认 `false`，
且 verdict=`duplicates` 也只是 flag 仍需人批）。反面对照：**Memanto 的 `map_zep`
把已失效的边直接跳过，不导入、不逐条列入报告**——那等于替用户裁决了；**mem0 默认的
`add(infer=True)` 由 LLM 判 ADD/UPDATE/DELETE，实测把矛盾句判成 ADD**——目标侧也在自动裁决（DEC-15）。

**理由**：铁律 7。语义去重只能产出候选，因为"这两条矛盾"这个判断本身可能错，
错了要能追溯和回退。

**后果**：

- **簇 id**：源本身有实体/属性结构（如 Graphiti 的边）时，用 Remnic 式 `entityRef::attribute`
  分组；自由文本（ChatGPT 保存的记忆、`MEMORY.md` 条目）没有这层结构，簇 id =
  `sha256(排序后的成员 canonical_id)`。两种都保证重跑得到同一个 id。
- **MVP 只做精确去重**（`content_hash` 与归一化文本哈希相同 → 后者 `omitted`，原因 `duplicate_of`）。
  语义聚类按 AGENTS.md 放在长期，且 embedding 必须本地（DEC-12）。
- **裁决动词作用于簇**：`keep(ids)` 保留子集，全选即「都对，只是语境不同」——这是最常见的情况；
  `needs-more-context` 暂不处理。**不做 `merge`**，连"人批的 merge"也不做（Remnic 有但默认关闭；
  合并不可逆，且等于替用户综合两条矛盾的记忆）。用户想要一条综合后的新记忆，就自己在目标里写。
- 裁决结果写进回执报告，下次运行读回来，同一个簇不再重复询问（DEC-18）。
- **未裁决的冲突不得用目标的 supersede/revise 表达**（UMP 的 `revise` 会造 successor，等于自动裁决）。
  人裁决之后，落选的那条可以 `omitted`，也可以用目标原生的 supersede 表达——这时它执行的是人的决定。

### DEC-7. conformance 套件 = 数据 oracle + schema，不 import 实现

**证据**：portable-memory 的切分方式值得抄——它的 **oracle 数据独立**（`canonical-json.json`
26 条向量、签名 fixture、JSON Schema，且这些文件在 **Swift 与 Python 两个仓库里字节相同**，
存在第二个独立语言实现交叉验证），但**执行器不独立**（`BundleValidator` 就是 SDK 的一部分）。
UMP 更差：**根本没有语言中立的独立向量目录**，`runConformance()` 只探 HTTP 端点、
**从不校验 L0 文件产物**。

**理由**：铁律 8。适配器作者不 merge 也能自证合规。

**后果**：套件 = JSON Schema + 测试向量 + 期望输出 + 一个**只读产物、不 import 引擎**的执行器。
能借 portable-memory 的**数据**，不能借它的 `BundleValidator`。
portable-memory 的 L2 层（写入后能被检索找回）自己也没给可执行的独立判定器（README 明说
live 断言"由 adopter 对自己的检索栈完成"）。我们是否为这一层提供独立探针，见 §6。

### DEC-8. UMP 默认走 L0 file binding，不做 DID/签名

**证据**：UMP（Universal Memory Protocol）的 L0/L1 **不要求 DID**（§5.1 明说可先用 opaque
`owner` 字符串），L0 的 MUST 只有两条：能 parse+emit 两种文件、导出时守 `consent.redact`。
但它有三个硬约束要注意：① `id` 必须是 `^urn:ump:[a-z2-7]+$`（**小写 base32**），
我们要加一层**可逆 id 桥接**（官方 Recall adapter 就是这么做的）；
② `*.ump.md` 的 front-matter **必须是紧凑 JSON 块**——规范样例给的类 YAML 形式
会让官方 `fromMarkdown()` 直接 `JSON.parse` 报错；③ 顶层 `additionalProperties: false`，
**没有 embedding 字段、没有通用 unmapped-field 容器**，`body.structured` 是唯一逃生口。

**理由**：MVP 不该为了导出目标引入身份基础设施。

**后果**：写 `*.ump.json` 数组 + 可选 `*.ump.md`，`owner` 用稳定 opaque 字符串，
`provenance.actor_kind="import"` + `provenance.source.provider` 标源，
把 embedding/模型/丢失字段/冲突清单全塞 `body.structured`。
**UMP 不给 bundle 级 checksum/manifest**（只有逐记录 blake3+ed25519 且仅 L3），
所以"每步产出机器可验证凭证"只能靠**我们自己的报告 + 逐记录 content_hash**。

### DEC-9. 网页端 Writer 的形态是可粘贴文本，不是文件格式

**证据**：Claude 与 Gemini 的记忆导入**都是粘贴文本框**，不是文件上传；ChatGPT 没有记忆导入入口。
Anthropic 官方给的导出 Prompt 要求格式 `[date saved, if available] - memory content`，并要求
"preserve my words verbatim"、"Do not summarize, group, or omit any entries"——
与我们 [reader-prompts.md](reader-prompts.md) 的 `[日期或未知] [类别] 内容` 只差类别字段。
**这是对"日期 + 内容"是事实标准的强验证。**

**理由**：目标平台的导入接口决定了 Writer 的形态。写一个格式漂亮但平台不吃的文件
等于没写。

**后果**：网页端 Writer 产出「按目标平台 Prompt 格式化的文本」，复用 `reader-prompts.md`
的格式；ChatGPT 不作为目标。**另有一个必须预警的目标侧行为**：Claude 官方自述导入
"experimental and still in active development"，且**可能不保留与工作无关的个人细节**
——这是**目标侧静默衰减**，发生在目标平台内部、我们无法回读，所以回执报告对这类目标一律
`unverifiable`，并声明"目标可能自行丢弃"。

### DEC-10. Reader 必须吃 ZIP

**证据**：ChatGPT 与 Claude 的官方导出**都是 ZIP**；Gemini 的聊天记录导入也是
上传 `.zip`（上限 5 GB，每天 5 个，**且官方文档直接教怎么从 ChatGPT / Claude 导出**，
即 Gemini 能吃这两家的官方导出包）。反面：**Remnic 直接拒绝 `.zip`/`.tar.gz`，
要求用户先手动解压**。

**理由**：这是真实迁移场景的第一步，把这一步推给用户是没必要的摩擦。

**后果**：第 1 阶段由引擎解压（解到临时目录，拒绝跳出目录的路径），再让已注册的 Reader
逐个声明认领哪些文件。导出包内部结构会变（ChatGPT 的 saved memories 有新旧两种形状），
所以源类型判定按"发现了哪些文件、内容长什么样"而不是"文件叫什么"。没有 Reader 认领的文件
进源清单，不静默忽略。

### DEC-11. dry-run 在管线层截断，不由适配器负责

**证据**：Remnic 的做法值得直接抄——`runImporter` 在 `dryRun` 时**直接 return，
永不调用 `writeTo`**，适配器无法绕过。反面：**Memanto 的 `--dry-run` 默认 `False`**，
即默认执行真实写盘。

**理由**：铁律 2。默认 dry-run 只有在**适配器无法绕过**时才是保证，否则就是文档约定。

**后果**：dry-run 是引擎层的强制分支；Writer 的写入方法只能由引擎在拿到审批凭证后调用。

### DEC-12. 抽取默认本地，远程模型必须显式 opt-in，调用事实结构化记录

**证据**：**Memanto 的 dry-run 也会调远程 Moorcheh 模型生成叙述**（`_render_savings_report`
→ `_generate_narrative`），非 opt-in，且调用事实只是报告里的文本（`llm_model`/`llm_method`）；
**Remnic** 的解析在本地，但它自己文档承认配了远程抽取模型时**内容照常外流**；
**claude-mem** 的默认形态就是每次工具调用都调 observer 模型。

**理由**：铁律 3。这是"记忆"这个品类的信任底线——用户的私人记忆不该因为跑一次
迁移就出网。

**后果**：

- 抽取用本地正则/规则/本地 NER。opt-in 远程模型时，**调用事实必须是结构化字段**
  （哪个模型、几次调用、哪些条、传了什么字段），不是散文。
- **目标侧触发的模型调用也要列**：mem0 写入要调目标配置的 embedding 模型，Graphiti 每个
  episode 要调多次 LLM 抽取（DEC-15）。这些不是我们发起的，但是迁移导致的，用户需要在计划报告里
  看到「写入会让目标调用模型 X 约 N 次」。

### DEC-13. 地址 ≠ 权限

**证据**：三处独立印证。① **A2M 规范自己警告 `owner` 是客户端自声明的**；
② **memcommons 的 I2 invariant 是「一个地址不是一项能力」**，把寻址与授权拆成两件事；
③ **IBM 草案规定 scope tags 由系统加、不由 agent 定义，并绑定到 ACL**，
且 `recall` 时"系统必须校验请求 scope 是否在 ACL 内"。

**理由**：铁律 4。从源系统读到的 `user_id`/`project`/`tenant` 只是**地址**——
它告诉我们这条记忆"关于谁/在哪"，不告诉我们"谁有权读"。

**后果**：canonical model 用 `scope`（地址）与 `owner_declared`（**标记不可信**）两个字段
分开表达。审批钩子**不得信任自声明 owner**，网络场景的权限必须由认证主体推导。
我们比 OMPI/IBM 更严一步：IBM 的 scope 由系统设、所以可信；源系统导出里的 owner 是**别人的系统**
设的，到我们这里只能当线索——报告里不能把"源系统说这是 alice 的"写成"这是 alice 的"。
memcommons 明说它 §4 把 consistency/wire schema/discovery 划出 v0.1，
所以它只是"寻址与权限语义"的参考，不是映射目标。

### DEC-14. 从 OMPI 盲区对读补进的字段

OMPI（Open Memory Protocol Initiative）的 `convergence-analysis.md` 是与我们
[memory-products.md](memory-products.md) **独立的第三方分析**，对读找出 14 项盲区，其中四项是真盲区：

| 字段 | 为什么需要 | 迁移里的具体用法 |
|---|---|---|
| `scope_qualifier` | 回答"哪个项目/哪个用户"。OMPI 原话 "needed but absent from most proposals" | 八套系统的身份字段八个答案，只能由管道赋值，所以单立字段 |
| `consent.memory_enabled` | per-scope 的同意开关；**scope 关闭时不得写新记忆** | 源侧某项目/会话关着记忆时，从它的聊天记录抽出的记忆默认不迁，计划报告列出由人决定；目标侧该 scope 关着时 Writer 拒写 |
| `manifest` | 源系统 / 导出时间 / schema 版本 | 进报告顶层（第 3 节） |
| `tombstone` | 导入时"先抹再合"，防复活 | 对上 DEC-2；portable-memory 有完整实现可参照 |

另外吸收：**记录时间与事实时间成对**（第 2 节时间层，原来只在 Graphiti 实测里见过）；
`auditable` 标记（可追溯到问责的人）；**逐对象导入回执的 5 态**（第 3 节）。

### DEC-15. 目标分四类形态，Writer 按类定策略

**证据**：[memory-products.md](memory-products.md) 的三类分法——文件式 / 数据库式 / 图谱式，
加上 DEC-9 的网页粘贴。落盘形态决定了写入策略和能验证到什么程度：

| 类 | 代表 | 写入策略 | 第 9 阶段能验证到 |
|---|---|---|---|
| 文件式 | basic-memory、OKF、UMP L0、Obsidian | 直接写文件/目录；**文件名由 `canonical_id` 派生** | 逐字节回读 |
| 数据库式 | mem0、Memoria、Panella | 走目标 API；**必须关掉目标侧 LLM 改写**（mem0 用 `infer=False`） | 按目标 id 回读 + 字段比对 |
| 图谱式 | Graphiti、Zep | 走 `add_episode` 交原文，让目标自己抽实体/边 | 只能检索探针抽查（抽取结果不确定） |
| 网页粘贴 | Claude、Gemini | 生成可粘贴文本（DEC-9） | 无法回读 → `unverifiable` |

**理由**：图谱式的目标是"给它原文、它自己抽"，我们不能替它抽（否则两套抽取逻辑打架）；
数据库式的目标是"给它结构化记录"；文件式的目标是"给它可读的目录"。
**同一份 canonical record 在这几类上的最优形状不同。**

**后果**：Writer 接口按类分组，但**报告 schema 统一**（第 3 节）。几个具体的坑：

- **幂等不能交给目标。** mem0 每次 `add` 都生成新 id；Graphiti 不对 episode 去重——我们自己的
  `lab/step3-graphiti/feed.py` 就得先查已入库的 episode 名再跳过，崩了重跑才不重复插；
  目标侧的 LLM 去重结果不确定。文件式也不能用 `content_hash` 当文件名，否则内容一改就多出一份。
  幂等统一靠我们自己的身份映射（DEC-16），跨次运行由上一次的回执报告带过来（DEC-18）。
- **mem0 的默认 `add(infer=True)` 不能用**：LLM 会抽取改写事实、自行判 ADD/UPDATE/DELETE
  （实测把矛盾句判成 ADD、把抽取时的当前时间织进事实），等于目标侧改写 + 自动裁决。
  `infer=False` 原文入库，但仍有三处要进报告：仍调目标的 embedding 模型；`role=system` 的消息
  被静默跳过、格式不对的消息只打 warning 跳过；OSS 版不支持 `timestamp` 参数，`created_at`
  会变成迁移时间（`mem0/memory/main.py` 的 `add` 文档字符串与 `_add_to_vector_store`）。
- **Graphiti 写入是重活**：每个 episode 多次 LLM 调用，实测 25–559 秒/episode 且随图增长
  （[PROGRESS.md](PROGRESS.md) Step 3）。计划报告要给调用量和耗时预估（DEC-12）。
- **Graphiti 不在查询里过滤失效边**：矛盾的边标 `invalid_at`，但已失效的边照样全量返回——
  "事实在 valid_at~invalid_at 之间有效"是写给下游 LLM 的提示词。所以"写进去了"不等于
  "目标会正确过滤"，检索探针要检查这一点。

### DEC-16. id 桥接是 Writer 的固定职责

**证据**：每个目标的 id 形状都不同——UMP 要求 `urn:ump:<小写 base32>`；
AIMEM 要求 `urn:aimem:<producer>:<local>`；portable-memory 用 `sec_`/`tomb_` 前缀；
其余用 uuid 或自增。**没有两家能直接对上。**

**理由**：`source_record_id` 必须原样保留（DEC-4 的可追溯性），所以目标 id 只能另造一层。

**后果**：canonical record 带 `canonical_id` + `source_record_id`，
Writer 负责生成目标 id 并**在回执报告里记下映射**（源 id ↔ canonical id ↔ 目标 id）。
这份映射同时是幂等的依据（DEC-15、DEC-18）。可逆桥接的做法可参照 UMP 官方 Recall adapter
（`urn:ump:<base32>` ↔ uuid）。

### DEC-17. Reader 交出源记录 + canonical 记录，引擎校验

**证据**：源的结构复杂度差别很大。ChatGPT 的 `conversations.json` 要沿 `mapping` /
`current_node` 遍历消息树，saved memories 有 `{memory:[...]}` 与顶层数组等新旧多种形状
（Remnic 的 importer 为此写了专门代码）；而 `MEMORY.md` 条目几乎是一行一条。
反面：Remnic 的 parser 用白名单 `normalize`，没声明的字段在解析阶段就丢了，没人能发现（DEC-4）。

**理由**：只靠声明式映射表表达不了消息树遍历这类结构，所以归一要放在 Reader 的代码里。
但 Reader 是社区贡献的插件，引擎不能盲信它的归一结果。

**后果**：

- Reader 交出三样东西：源记录（原样字段）、canonical 记录、未承载字段清单。
- 引擎做两项校验：canonical 记录符合 schema；源记录里的每个字段，要么出现在字段映射表里，
  要么出现在未承载字段清单里，**两边都没有的字段由引擎标出**。这样 Remnic 式的白名单丢字段
  在引擎层就会暴露。
- AGENTS.md 的模块边界相应写成「Reader：解析 + 归一；引擎：校验 → 闸门 → …」。

### DEC-18. 上一次的回执报告是下一次运行的输入

**证据**：DEC-15 已经说明幂等不能交给目标；DEC-2 的防复活需要知道「哪些记录已经写过、
后来被用户删了」；DEC-6 的裁决结果需要跨次保留，否则每次重跑都重复询问。
这三件事都需要跨次运行的状态。

**理由**：回执报告本来就带着全部所需信息——源 id ↔ canonical id ↔ 目标 id 的映射、
`content_hash`、验证结果、裁决结果。拿它当状态，就不用另建账本（守住「不自建治理账本」），
状态也和用户审过的东西是同一份文件。

**后果**：重跑时引擎读入上一次的回执报告，对每条源记录：

| 情况 | 处置 |
|---|---|
| 同一源 id、完整 `record_hash` 不变且目标未被改动 | `omitted`，原因 `already_migrated` |
| 同一源 id、正文或元数据变了且目标未被改动 | 按目标能力更新，计划报告列出新旧哈希 |
| 上次 `verified`，这次回读发现目标里没了 | 视为用户在目标侧删除，`omitted`，原因 `deleted_in_target`，**不重写**（防复活） |
| 上次是 `unverifiable` 的目标（网页粘贴） | 无法判断是否被删，计划报告提示用户自己确认 |
| 簇在上次报告里有裁决 | 沿用裁决，不再询问 |
| 目标正文或 canonical 元数据被用户改动 | `unresolved target_modified`，不自动覆盖；家模式（OKF 家目标）下本行由 DEC-21 取代——可解析的用户编辑不落在 `target_modified`，按字段级规则与四规则处置（可能是 `omitted home_modified`，双边不同则是冲突候选 `unresolved`），其余目标维持 `target_modified` |
| 目标同名条目存在但无历史依据 | `unresolved target_untracked`，不自动覆盖 |

M3 已确认：最新回执的 `prior_write` 携带此前真实写入与验证；本次 skipped 条目不写本次 verification。
重复引用和暂时从源消失的条目同样保留这份依据。最新一份回执即可保护历史状态，不依赖完整旧链。
旧回执条目必须关联唯一的目标声明，并核对当前目标位置与 Writer；没有关联声明不能只发警告后仍利用历史授权覆盖。
计划摘要的 `previous_receipt_hash` 绑定实际加载的完整旧回执字节，包括缺源条目、共享快照与裁决。
批准后这些依据变化，执行拒绝；执行使用已核对的内存快照，不边写边采样可变旧文件。
M5 已确认：`prior_write.target_hash` 额外覆盖原生记录载荷；共享产物的路径、哈希和字节数
进入目标快照。以 Writer 预计回读结果比较历史，避免明确移除向量后的重复更新。
没有 verified 写入时，回执保留此前共享产物依据，不把受阻运行期间观察到的修改当作新写入。

回执链按「源 → 目标」一对一份。家模式下每颗卫星只对家一条链（N 颗卫星 N 条，
而不是两两同步的 N² 条），存放在家目录里随家走（DEC-19）。

没有上一次回执报告时按首次迁移处理；用户也可以显式要求忽略旧报告、全量重来。
这两种情况下防复活都会失效（目标里已存在的同源记录会再写一份，用户删过的会被写回），
所以只要目标里已有数据，计划报告就要把这一点写成显式警告。

### DEC-19. 家模式：用户选家，默认 OKF 目录，先只做「卫星 → 家」

**证据**：

- **多工具并用是常态，记忆会分叉。** Harness 侧记忆已收敛成三种本地形态
  （[source-memory-formats.md](source-memory-formats.md)）；Claude Code 与 Codex 各有一套 `MEMORY.md`。
  手工解法已经存在：一份正本 `AGENTS.md`，`CLAUDE.md`/`GEMINI.md` 做软链接指向它——
  这就是「家 + 卫星」，而且家是普通 Markdown，没有发明任何格式。
- **中心存储就是记忆产品本身。** Memanto、Remnic、mem0 都是「多源 → 自家中心存储」。
  我们自建一个，就成了功能更少的 Remnic，也和 UMP、portable-memory 这些可携带存储格式正面竞争。
- **一对一回执的规模问题。** DEC-18 的回执链是一对一份，N 个工具两两同步要 N² 条链，
  也没有一个地方能看到跨工具的全局冲突。
- **OKF v0.2 适合当家**（`lab/upstream/open-knowledge-format/SPEC.md`）：唯一必需字段是 `type`（§11）；
  扩展字段 Producers MAY 任意添加、Consumers SHOULD 往返保留且 MUST NOT 拒收（§4.1）；
  推荐用 git 分发（§3）；Concept ID 就是文件路径（§2）；有 `log.md` 变更记录（§9）、
  `status: deprecated`（§5.4）、`sources` 溯源（§5.1）、`generated`/`verified` 与 `human:` 前缀的 actor 约定（§5.2、§7）。
- **但 OKF 自己也会破坏性变更**：v0.1 → v0.2 把 `timestamp` 换成 `generated.at`、
  把正文 `# Citations` 挪进 `sources`（§13.1）。而真实 Consumer 也不一定守「保留未知字段」：
  Memanto 的 `okf_loader` 把未知 frontmatter 塞进正文尾部，受 800/200 字符截断（DEC-4）。

**理由**：用户要的是两件事——工具长期有用、数据不被脆弱的格式绑架。家模式满足第一件；
默认家选「没有我们、没有 OKF 工具也能直接读」的纯 Markdown + YAML，满足第二件：
最坏情况下 OKF 没人维护了、我们也没了，用户手里仍是一个能读、能 grep、有 git 历史的目录。
自建存储两件都不满足：它让数据依赖我们，且一旦有人长期存数据，它就成了事实格式（Hyrum 定律）。

**后果**：

- **角色**：家是用户指定的一个系统（默认 OKF 目录，也可选 basic-memory、portable-memory、mem0 等）；
  其余工具都是卫星。换家本身就是一次「旧家 → 新家」的直迁。
- **方向**：MVP 只做**卫星 → 家**（汇总）。每次汇总就是一次目标为家的迁移，DEC-1 到 DEC-18 全部适用
  （闸门、dry-run、审批、回读、回执）。**家 → 卫星**（分发）以后再做，冲突面更大。
- **家也是源**：要有能读回家目录的 OKF Reader。汇总时卫星的新记录和家里已有记录一起去重聚类，
  跨工具的冲突在这里第一次被放到一张清单上。
- **卫星里消失的记录默认不动家**：卫星侧删除可能只是用户清理了某个工具，不等于不要这条记忆。
  计划报告列出，由人决定在家里 `status: deprecated`（保留式）还是移除（墓碑式，见 §6 Q6）。
- **默认家的目录约定**：

  ```
  home/                              # 一个 git 仓库
    index.md                         # 只放 okf_version: "0.2"，正文按 scope 分组列出记忆
    log.md                           # 每次汇总一条：日期、卫星、增/改/弃计数
    .mem-adaptor/config.toml         # init 时写入：闸门策略（DEC-1）等；卫星登记表在 apply 批准后追加（DEC-20）
    memories/<canonical_id>.md       # 平铺；路径即 Concept ID，不编码 scope 等可变属性
    .mem-adaptor/receipts/<卫星 ID>/<运行 id>.json   # 回执报告，随家走（换机器不丢防复活状态）；卫星身份见 DEC-20
    .mem-adaptor/plans/<卫星 ID>/<运行 id>.json      # 家模式计划报告的默认落位；`.mem-adaptor/` 不是记忆源，读家目录时排除
  ```

- **字段落位**：能用 OKF 标准字段的就用标准字段，其余进一个命名空间扩展块；
  **人离开扩展块也必须能读懂每条记忆**，因为扩展块可能被别的 Consumer 截断或丢弃。

  | canonical | 默认家里的位置 |
  |---|---|
  | `content` | 正文，**原文不改** |
  | — | `type: Memory`（固定值，方便 Consumer 路由） |
  | `title` / `description` | 由正文派生，只作展示；原文以正文为准 |
  | `source.*` / `source_locator` / 源 actor | `sources: [{id, resource, author, last_modified}]` |
  | `updated_at`（源侧） | `generated.at`；`generated.by` 写**源侧作者**（用户手写的用 `human:`），不写我们 |
  | 裁决落选、人决定弃用 | `status: deprecated` |
  | 其余（`source.satellite_id`（M6 新增，DEC-20）、`scope`、`scope_qualifier`、`owner_declared`、`dna_class`、`source_record_id`、`evidence_level`、`consent.*`、embedding 的 `model`/`dim`、`conflict_cluster_id`） | 扩展块 `mem_adaptor:` |

- **不写 `verified`**：OKF 的 `verified` 表示「对照来源确认过内容」，而用户批准一次迁移
  不等于核实了每条事实。写了会让 Consumer 把它算成 human-reviewed 级，属于夸大可信度。
- **向量不进家**：家是文本正本，向量绑定模型且人读不了；只在扩展块记源侧的模型名和维度，
  检索层自己嵌入（报告里写明，符合 DEC-5）。
- **固定 OKF 版本**：根 `index.md` 声明 `okf_version`，Writer 按声明的版本写；
  OKF 升版时由 Writer 生成一次「旧版 → 新版」的计划报告，像普通迁移一样审批后执行。
- **家和回执都是敏感数据**：家目录推到公开远端等于公开全部记忆，文档和 CLI 首次运行要明确提示。
- **卫星身份与家编辑规则**：卫星编号的发放、登记与回执绑定见 DEC-20，家目录里用户编辑的归属与四种变化规则见 DEC-21；本节的目录约定与字段落位保持有效。

### DEC-20. 卫星 ID 身份契约：身份锚定卫星编号，不锚定路径

**证据**（#19 试点 review 包，见 [PR #20 评论](https://github.com/kalinplus/mem-adaptor/pull/20#issuecomment-6029980022)，探针复现）：

- **同名路径撞身份**（R4）：`canonical_id` 现由 `(system, source_record_id)` 两段派生
  （`crates/core/src/engine.rs:38-46`、`crates/core/src/reader.rs:37`），本地 Markdown 的 `source_record_id` 是相对路径。
  两个 vault 里同路径笔记身份相同；第二颗卫星汇入同一个家时，条目变成 `unresolved target_untracked`，
  或带旧回执被 `Previous receipt belongs to another source` 拒绝。结果是保守拒绝不是静默覆盖，但多卫星汇总被卡死。
- **回执按绝对路径绑定**：旧回执归属核对是 `receipt.source.location == source.root` 的字符串比较
  （`crates/core/src/engine.rs:266-269`）。源目录一搬家，更新、防复活、裁决复用（DEC-18）的历史链全部丢失。
- **`scope_qualifier` 没有卫星维度**：R11 曾嵌入绝对路径，#21/PR #23 改为相对父目录
  （`crates/reader-markdown/src/lib.rs:158-174`），跨卫星的同名项目仍然无法区分。
- **导出包路径天然不稳定**：ChatGPT / Claude 的 ZIP 每次下载路径都不同，按路径锚定身份等于每次导出都是新源。

**理由**：身份需要一个不随目录移动而变、又能区分同名来源的锚点，路径两样都不满足。
卫星编号（satellite ID）由家的登记表发放一次、永不改变，路径只是当前可解析的绑定，可随时重绑。
登记表放家里随家走，与 DEC-19「回执存家里」一致；它只是绑定登记，不是治理账本——
只记 id ↔ 标签 ↔ 路径，不记迁移状态，状态仍全部在回执里（DEC-18）。

**后果**：

1. **身份公式**（替代 [implementation-plan.md](implementation-plan.md) §2 的两段公式）：
   `canonical_id = sha256(system ‖ 0x00 ‖ satellite_id ‖ 0x00 ‖ source_record_id)`，取前 20 字节转无 padding 的
   小写 base32（RFC 4648 Base32 编码），共 32 字符；schema 的 `^[a-z2-7]{32}$` pattern 不变，变的是派生输入。
   直迁模式 `satellite_id` 为空串、仍参与哈希（统一公式，不做条件分支），因此直迁的 id 值也与旧公式不同。
   项目未发布：已写过的家与全部测试 golden 重新生成，不做兼容。
2. **id 只算一次，随记录走**：`canonical_id` 只在第一次从卫星读入时计算；canonical 记录新增可选字段
   `source.satellite_id`（直迁缺省），随记录存进家里的 `mem_adaptor:` 扩展块。从家读回时以扩展块存量为准，
   **不重算**——`crates/core/src/okf.rs:79-83` 的重算核对删除，代之以 schema 格式校验和与该记录所属卫星链的一致性核对。
   家本身不领卫星编号；家记录保留其原卫星的编号。
3. **短编号格式与生成**（Issue #22 待设计项，本条定案）：
   - 字符集 `[a-z2-7]`，与 `canonical_id` 同一字母表（无 0/1/8/9 形近字符）；定长 8 字符（2^40 空间），
     是安全的单路径段，且定长 8 位不会撞上 Windows 保留设备名（CON、COM1 等）。
   - 生成是确定性派生：`satellite_id = base32(sha256("mem-adaptor:satellite:v1" ‖ 0x00 ‖ <首次登记时的规范化绝对路径>)[..5])`
     转小写。域分离前缀避免与其他哈希互撞；不含时间戳，所以同一未登记路径在 plan 与 apply 两阶段派生出同一候选 id，
     计划摘要稳定（DEC-3）。
   - **同一家内不重复由登记表保证，不靠概率**：写入前与登记表全部现有 id 核对；派生 id 已被别的路径占用时
     按第 6 条的搬家/复用流程处理，不静默换号。
   - 另有显示标签（label）：可改、只用于展示，默认取登记时的目录名，直接编辑登记表即可；不进任何哈希，不要求唯一。
4. **登记表**：`.mem-adaptor/config.toml` 新增卫星条目（`id`、`label`、`path` 可选、`system`、`created_at`）。
   **只在 apply 批准后写入，plan 阶段只读不写**（dry-run 不落盘，DEC-11）。导出包类卫星没有 `path`。
5. **解析顺序**：显式 `--satellite <ID>` > 登记表按当前路径匹配 > 派生候选新 id。同一路径再次汇总自动命中登记表；
   源目录搬家后用 `--satellite <ID>` 重绑登记表路径。导出包类来源（ChatGPT / Claude 的 ZIP）每次必须显式 `--satellite`：
   已有编号延续旧链，`--satellite new` 新建卫星（label 默认取文件名，不登记 path）。
   能否按导出内的账号标识自动匹配卫星，等 M7 用真实导出核实后再议（§6 Q10）。
6. **搬家检测与「大部分吻合」判定标准**（Issue #22 待设计项，本条定案）：新路径未登记、派生出候选 id 时触发。
   对每颗已有卫星的最近一份有效回执：单条「吻合」= `source_record_id` 相同且 `content_hash` 相同；
   若 `吻合条数 ÷ 本轮新源记录条数 ≥ 0.6` 且 `吻合条数 ≥ 5`，判定为疑似搬家/路径复用。
   交互模式警告并要求用户在「重绑到该卫星」与「作为新卫星继续」之间确认；非交互模式拒绝，提示先用 `--satellite <ID>`。
   **只提示，不自动合并**。少于 5 条的小源不触发提示，重复内容由去重聚类兜底、交人裁决（DEC-6）。
7. **回执按卫星 ID 绑定**：家模式下旧回执归属核对改为 `receipt.source.satellite.id == 当前卫星 ID`，
   替代 `crates/core/src/engine.rs:266-269` 的绝对路径比较；`source.location` 保留为展示与审计字段。
   直迁没有卫星，location 比较照旧，行为与现在一致。回执目录 `.mem-adaptor/receipts/<卫星 ID>/` 不变（DEC-19）。
8. **Claude Code 的 `scope_qualifier`** = `<卫星 ID>/<相对父目录>`（父目录为空时只有卫星 ID；
   直迁维持现状的相对父目录）。该值写入扩展块、从家读回时恢复，不随恢复重算。
9. 与 [m6-cli-proposal.md](m6-cli-proposal.md) §2 的关系：显式 `--satellite`、回执/计划按卫星分目录的推荐保留；
   其中「不会因此修改已确认的 canonical id 算法」一句被本 DEC 取代（Issue #22 用户确认），
   「安全的单路径段标识」落实为第 3 条的 8 字符 base32。

### DEC-21. 家目录编辑策略：归属按扩展块存量证据，四种变化规则按上次写入基准

**证据**（#19 R1，[PR #20 试点 review 包](https://github.com/kalinplus/mem-adaptor/pull/20#issuecomment-6029980022)探针复现）：

- 在家里改正文（原笔记有 `title` 时）：整个计划失败于 `Conflicting original and envelope source metadata`
  （`crates/core/src/reader.rs:260-263` 的 `source_extra` 合并冲突）；无 `title` 时不失败，但 Writer 自己生成的旧标题
  被误列为 `source_unknown` 未知字段。改 `tags`：整个计划失败——`okf::validate_projection` 要求 frontmatter
  等于派生值（`crates/core/src/okf.rs:125-134`），Reader 在 `crates/reader-markdown/src/lib.rs:93` 调用它，
  Writer 侧 `inspect` 也走同一校验（`crates/writer-okf/src/lib.rs:396-416`）。改 `title`：被误报为「来源未知字段」
  ——`crates/reader-markdown/src/lib.rs:108-110` 只在值等于派生值时才映射该字段。
- 变异测试佐证：正文变更判断的 `!=` → `==` 变异存活（review 基线 443e799 的 `lib.rs:88:36`，
  现对应 `crates/reader-markdown/src/lib.rs:94`）——没有任何测试编辑过家目录正文。
- 根因是同一个：**字段归属靠猜**——「当前值是否等于由当前 canonical 记录派生的值」（`native_projection`，
  `crates/core/src/okf.rs:101-122`）。派生值随正文变化漂移，猜测机制随即失效，用户编辑被当成损坏数据或未知字段。

**理由**：归属不能靠猜，要靠存量证据。`mem_adaptor:` 扩展块本来就存着整条 canonical 记录
（除正文外，`crates/writer-okf/src/lib.rs:244-245, 284`）：`tags`、`sources` 各字段、`generated` 的「工具写入值」
直接由扩展块存量给出或唯一确定；正文变化有扩展块 `content_hash` 这个独立判据。唯一有歧义的是 `title`
（由正文派生，正文被编辑后分不清「用户改的」与「正文更新后残留的工具旧值」）——但两种情形的处置完全一致
（保留、不覆盖），歧义不影响任何决定，所以不需要另存字段快照。家的定位已确认为可维护的主库
（Issue #22，2026-10-07）：用户的正常内容更新必须被保护，四种变化规则已确认；本 DEC 把它们落实为可判定的
字段规则与比较基准。边界不变：**受管文件解析失败仍整体失败**（损坏的受管声明不能静默跳过，
[m4-reader-proposal.md](m4-reader-proposal.md) 契约）；**可解析的用户编辑**走下面的字段级规则，
单文件编辑不中止整个计划，也不静默丢弃。

**后果**：

**A. 字段级规则**（家读回时的归属判据与处置；归属判据取代值相等猜测）：

| 字段 | 归属判据（与扩展块存量比较） | 用户改动的处置 |
|---|---|---|
| 正文 | `content_hash(body)` ≠ 扩展块记录的 `content_hash`（现有 `okf_body_changed` 异常保留，`crates/reader-markdown/src/lib.rs:94-97`） | **新事实**：读回采用家正文，`content_hash` 以家值为准；是否写回家由 B 的四规则决定 |
| `tags` | frontmatter 值 ≠ 扩展块记录的 `tags`（无歧义） | **新事实**：读回的 `canonical.tags` 取家当前值；卫星侧也改 → 冲突候选，不自动并集 |
| `title` | 不等于由正文派生的显示值（`crates/core/src/writer.rs:36-40`） | **只提示、粘性保留**：纯展示字段（canonical 无 title 字段），一律当用户显示值处理，不区分残留旧值；不进 unmapped/`source_extra`（修复 R1 误报），不失败；后续批准更新也不以派生值覆盖，直到用户改回派生值或删除 |
| `sources` / `generated` | frontmatter 值 ≠ 由扩展块字段（`source_record_id`、`source_locator`、`provenance.actor`、`updated_at`）派生的值（无歧义） | **只提示、不采纳**：溯源是事实记录，不是用户可创作的内容；canonical 以扩展块为准，文件当前值保留不覆盖（粘性），报告列异常 |

`mem_adaptor:` 扩展块是工具专属区，用户改动分三种情况：

- **整体删除**：文件降级为用户普通笔记（新身份，按普通 Markdown 读取）；原卫星链中该 `canonical_id` 的条目
  在计划报告列为 `unresolved target_unmanaged`（新原因：目标已脱管），不自动覆盖、不自动写回，交人裁决。
- **修改后仍能通过 schema 校验**：以恢复值为准读回；引擎按回执链核对——`canonical_id` 不在任何链中按
  `target_untracked` 处理，在链中则作为「家变了」进入 B 的四规则。
- **修改到校验不过**（可解析但 schema/一致性不通过）：记异常 `managed_envelope_invalid`，该文件在计划报告中
  列为 `unresolved` 条目，其余文件与整个计划继续。YAML 解析级失败维持现有行为：整体失败。

**B. 四种变化规则与回执契约**（比较基准 = 该卫星最近一份有效回执中该条目的 `prior_write`；
不新增状态账本，也不新增回执字段）：

- 「**卫星变了**」：本轮卫星侧 canonical 记录的 `record_hash` ≠ `prior_write.record_hash`。
- 「**家变了**」：家文件当前整文件字节哈希 ≠ `prior_write.target_hash`（`target_hash` 已经是整文件字节哈希，
  `crates/writer-okf/src/lib.rs:418-424`）。字段级定位只用于报告展示，用 A 的归属判据得出。

| 卫星 vs 上次写入 | 家 vs 上次写入 | 处置 |
|---|---|---|
| 没变 | 没变 | `omitted already_migrated`，不重复写入 |
| 变了 | 没变 | 列入更新计划（列出新旧 `record_hash`），批准后写入家 |
| 没变 | 变了 | `omitted home_modified`（新原因）：保留家的修改，不写入；报告列出被改字段（`home_changed_fields`，载体与理由见后果 E）；不算失败 |
| 变了 | 变了且双方不同 | **冲突候选**：保留双方候选值，`unresolved`，由人裁决，不自动合并（铁律 7） |
| 变了 | 变了但家当前状态与本轮卫星投影完全一致（整文件字节哈希相等） | `omitted already_migrated`（已收敛，无需写入） |

- **比较按记录级，不做字段级合并**：字段级合并就是自动合并（铁律 7 禁止），所以即使双方改动落在不同字段
  （如家改 `tags`、卫星改正文），也进冲突清单由人二选一，不自动拼装。
- **基准不随跳过推进**：家只变的轮次不发生写入，`prior_write` 不推进。这是有意的——只有批准写入才推进基准，
  否则「家先改、卫星后改」会被洗成「只有卫星变」而覆盖用户编辑。
- **裁决**：每簇两个候选（卫星值 / 家值）。取卫星值 = 批准后写入；取家值 = 回执把家当前状态记为新的
  `prior_write`（回读验证 `verified`，字节不变）。裁决结果写进回执 `verdicts`，重跑沿用（DEC-6、DEC-18）。
- **删除语义另议**（§6 Q6）：卫星缺席与家文件删除都不当空正文，`source_missing` 与 `deleted_in_target`
  防复活照旧（§0 增量汇总、DEC-18）。
- 跨卫星共享产物（`index.md` / `log.md`）的对账按 [m6-cli-proposal.md](m6-cli-proposal.md) §1 已确认的推荐执行，本 DEC 不重复。

**C. 冲突候选的呈现**：复用 DEC-6 的 `conflict_cluster_id` / `conflict_candidates` 结构，簇 basis 为
「上次写入后双方变化且不同」；每个候选列出来源（卫星 ID + 标签 / 家文件路径）、`content_hash`、`record_hash`。
报告不含正文（DEC-1），要看内容由用户按 locator 自行打开文件。交互式 `apply` 逐簇列出由用户选择；
非交互模式这些条目保持不写并列进回执（M6 退出码约定：`unresolved` 不算成功）。

**D. 实现侧约束**：`okf::validate_projection` 与 Writer 的 `inspect`（`crates/writer-okf/src/lib.rs:396-416`）
从「发散即失败」改为「归属分类」；所有权检查、目标快照与审批时重核（同文件 `:254-264`，批准后目标变化 → 拒写）
的语义不变，用户编辑的发散判断从 Reader/Writer 失败前移到引擎的四规则比较。

**E. schema 变更方案**（正本届时随实现 Issue 修改，本设计轮不动 `schema/`）：

- `schema/canonical-record.schema.json`：`$defs/canonical_id` 的 pattern `^[a-z2-7]{32}$` **不变**，
  变的是派生公式（DEC-20 第 1 条），公式不在 schema 里表达；`$defs/source_identity` 新增可选
  `satellite_id`（`{"type": "string", "pattern": "^[a-z2-7]{8}$"}`），`required` 列表不变（直迁缺省）。
- `schema/plan-report.schema.json`：
  - `$defs/source` 新增可选 `satellite` 对象（`{"id": pattern ^[a-z2-7]{8}$, "label": 非空字符串}`，
    `additionalProperties: false`，`required: ["id"]`）；`location` 保留为展示与审计字段，直迁缺省 `satellite`。
  - `$defs/omission_reason` 扩展 `home_modified`（B 表「只有家变」）。现有各分支 `additionalProperties: false`
    且无载荷分支的枚举是封闭的，必须显式加：`home_modified` **单独成一个带载荷的分支**
    `{"required": ["code", "home_changed_fields"], "properties": {"code": {"const": "home_modified"},
    "home_changed_fields": {"type": "array", "minItems": 1, "items": pointer}}, "additionalProperties": false}`。
    `home_changed_fields` 就是 B 表「报告列出被改字段」的载体：用户在家改动的 canonical 字段路径清单，
    由 A 的归属判据得出。**放 omission 分支而不是 anomalies 的理由**：`omission_reason` 的 oneOf 已有按原因
    带载荷的先例（`duplicate_of` 带 `canonical_id`、`target_unsupported` 带 `field`、`verdict_excluded` 带
    `cluster_id`），条目自带 `target_map` 定位家文件，报告自包含；`anomaly` 结构是源侧取向的
    （`source_locator`/`line`），不适合承载目标侧字段变化。字段清单随条目 disposition 进计划摘要
    （digest_inputs 的 predictions），审批绑定的就是用户看到的被改字段（DEC-3）。
  - `$defs/unresolved_reason` 的无载荷分支枚举加 `target_unmanaged`（A 的扩展块「整体删除」态），
    不带附加载荷——条目的 `target_map` 已足够定位。
- `schema/receipt-report.schema.json`：无结构变更——其 `source` 与 `disposition` 都 `$ref` plan-report 的
  `$defs`（`receipt-report.schema.json:14, :37`），上述枚举与分支扩展自动生效；`prior_write` 也不加字段
  （`target_hash` 已覆盖整文件，见 B）。
- `schema/config.schema.json`：变更方案文字见 [implementation-plan.md](implementation-plan.md) M6。
- `schema/vectors/`：`valid/canonical-full.json` 补 `source.satellite_id`；`valid/plan.json`、`valid/receipt.json`
  的 `source` 补 `satellite`，并各补一条 `omitted home_modified`（含 `home_changed_fields`）与一条
  `unresolved target_unmanaged` 条目；`valid/config.json` 补 `satellites`；`invalid/cases.json` 补反例
  （`satellite_id` 非法字符集/长度、`satellite` 缺 `id`、`satellites` 条目缺字段、`home_modified` 缺
  `home_changed_fields`），并注明各自违反哪条约束。

## 5. 明确不做的事

| 不做 | 为什么 |
|---|---|
| 自建中心存储 / 本地档案格式 | DEC-19。家由用户在现有格式里选。**回头重议的条件**：实践中发现用户必须长期保存的东西（如完整溯源链、向量）连现有格式的扩展槽都放不下——那是一次铁律 1 的修改 |
| 交换格式规范 | 铁律 1。名字通胀是前车之鉴：AMP v0.1 → UMP、MIF 从 0.1 到 1.0 把 AI memory 降级成"一个 profile"、aimem 这个名字撞了三个不相关仓库、"Open Memory Protocol" 有两个无血缘项目 |
| 自动冲突裁决 | 铁律 7。DEC-6 给了理由：判断本身可能错，且 merge 不可逆 |
| 自建治理账本 | 治理是可插拔后端（Panella 式哈希链 / Memoria 式 CoW），我们只留接口 |
| import 引擎的 conformance 执行器 | 铁律 8、DEC-7。执行器只读产物与数据，不 import 引擎和适配器 |
| SaaS 化 | 定位是本地施工队 |
| 替图谱式目标抽实体/边 | DEC-15。两套抽取逻辑会打架 |
| 支持 DID/签名的默认路径 | DEC-8。L0 够用，升级是显式动作 |
| ChatGPT 作为写入目标 | DEC-9。它没有记忆导入入口 |

**一条策略上的正面参照**：MIF 靠"填 OKF 故意留白的部分"活下来（ADR-009 pin OKF v0.1
conformance、无 normative 依赖）。这验证了我们的定位策略——**做映射器而不是做规范**，
靠别人留下的空位生存。

## 6. 待定问题

| # | 问题 | 牵涉 | 现状 |
|---|---|---|---|
| Q1 | 密钥与 PII 的闸门策略；密钥闸门是否提前进 MVP D1 | DEC-1 | 已定（已修订）：检测常开；密钥、高危 PII 默认放行，init/首次运行让用户选；D1 做密钥正则 |
| Q2 | Reader 契约：Reader 产出 canonical，还是引擎按声明式映射表归一 | DEC-17 | 已定：Reader 产出，引擎校验 |
| Q3 | 跨次运行的状态（重跑、增量、防复活） | DEC-18 | 已定：上一次的回执报告作输入 |
| Q4 | `merge` 是否保留为显式人批动作 | DEC-6 | 已定：不做 |
| Q5 | conformance 是否为「写入后能被目标自己的检索找回」提供独立探针，MVP 是否做 | DEC-7 | 未讨论 |
| Q6 | 删除语义到各目标的具体映射规则（含家里「保留式 deprecated / 墓碑式移除」怎么选） | DEC-2、DEC-19 | 未设计 |
| Q7 | 定位：一次性搬家，还是加上持续同步 | DEC-19 | 已定：家模式，默认 OKF 目录 |
| Q8 | 家 → 卫星的分发（双向同步） | DEC-19 | 已定：MVP 不做，以后再议 |
| Q9 | 远程去向（Mem0 云、网页粘贴、opt-in 远程模型）是否单独一档闸门策略、默认拦截 | DEC-1 | 未定：MVP 一个策略管所有去向，D2–3 接 Mem0 Writer 时再议 |
| Q10 | 导出包类来源能否按导出内的账号标识自动匹配卫星 | DEC-20 | 未定：等 M7 用真实导出核实后再议；当前每次显式 `--satellite` |

## 7. 证据索引

| 结论 | 出处 |
|---|---|
| Remnic 10 条静默衰减、dry-run 一行、pairId、复合键 | [ecosystem.md](ecosystem.md) 的 Remnic 实测发现；`lab/upstream/remnic/packages/import-*` |
| Memanto footer 800/200、`expires_at` 静默丢、默认非 dry-run、dry-run 调远程模型 | [ecosystem.md](ecosystem.md) 的 Memanto 实测发现；`lab/upstream/memanto/memanto/cli/migrate/mappers.py`、`memanto/app/core.py` |
| `secretRef` 明文密文 NEVER 随包 | [ecosystem.md](ecosystem.md) 的 portable-memory 实测发现；`lab/upstream/portable-memory/Schemas/secretRef.schema.json` |
| `tombstone` 物理删 + proof-of-reach + no resurrection | 同上；`Schemas/tombstone.schema.json`、`Spec/portable-memory-spec.md` §5 |
| conformance 数据独立 / 执行器不独立 | 同上；`portable-memory/Conformance/` |
| UMP L0 不要求 DID、id base32、`*.ump.md` 必须 JSON front-matter、无 embedding 字段 | [ecosystem.md](ecosystem.md) 的 UMP 实测发现；`lab/upstream/universal-memory-protocol/SPEC.md`、`src/bindings/file.ts` |
| UMP `forget` 默认保留记录、`hard` 只换正文 | `lab/upstream/universal-memory-protocol/src/server.ts:271-312` |
| UMP `requireValidSignature` 是死代码、默认只注入 private | [ecosystem.md](ecosystem.md) 的 UMP 实测发现；`src/rehydrate.ts:52` |
| OMPI 8 条分歧轴、最小可行标准六节、14 项盲区、5 态回执 | [ecosystem.md](ecosystem.md) 的 OMPI 实测发现；`Open-Memory-Protocol/prior-art/convergence-analysis.md` |
| IBM 草案 scope→ACL、记忆不可变、必须有 version comparator | 同上；`early-draft-specs/draft-v0.1-ibm.pdf` |
| AIMEM DNA 五类、embedding 模型名+维度、禁止静默重嵌入 | [ecosystem.md](ecosystem.md) 的 AIMEM 条目；IETF draft §2.2/§2.7/§5.1 |
| 八套系统身份字段互不兼容、无一家记 embedding 模型名、PII 拦截四连零 | [PROGRESS.md](PROGRESS.md) 五问表；[step4-5-report.md](step4-5-report.md) 逐字段矩阵 |
| mem0 `infer=True` LLM 改写与 ADD 误判、`infer=False` 跳过 system 消息、OSS 不支持 `timestamp` | [memory-products.md](memory-products.md) 的 mem0 节；`lab/step2-mem0/.venv/.../mem0/memory/main.py`（`add`、`_add_to_vector_store`） |
| Graphiti 不对 episode 去重、每 episode 25–559 秒、失效边照样返回 | [PROGRESS.md](PROGRESS.md) Step 3 与五问表第 4 问；`lab/step3-graphiti/feed.py` |
| Claude/Gemini 导入是粘贴文本、Anthropic 官方 Prompt 格式、ChatGPT 无导入 | [ecosystem.md](ecosystem.md) 的「平台原生导入通道」；[source-memory-formats.md](source-memory-formats.md) |
| 三类形态（文件式/数据库式/图谱式） | [memory-products.md](memory-products.md) |
| OKF v0.2：只有 `type` 必需、扩展字段须保留不拒收、git 分发、Concept ID 即路径、v0.1→v0.2 破坏性变更 | `lab/upstream/open-knowledge-format/SPEC.md` §2、§3、§4.1、§5、§7、§9、§11、§13.1 |
| R1 家编辑探针（正文/tags/title 三症状）、R4 同名路径身份冲突、R11 绝对路径 `scope_qualifier`、`88:36` 存活变异 | [PR #20 试点 review 包](https://github.com/kalinplus/mem-adaptor/pull/20#issuecomment-6029980022)；GitHub Issue #19、#21、#22 |
