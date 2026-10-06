# M4：三个 Reader 的实现方案（已确认、已实现）

用户已确认两项推荐：`profile/preference/instruction → dna`，其余 `standard`；
内部模型增加可选 `source_extra`，纳入记录摘要和密钥检测。

M4 已完成合成验收，验收时 62 个测试、构建、格式和 Clippy 通过；依赖方向保持不变。
本方案只处理本地或用户提供的导出文件；不访问真实记忆目录，不请求远程模型，
不把聊天记录自动抽取成记忆。

## 1. 记忆保护等级映射

按 implementation-plan.md 已给出的默认表：

| 源类别（原样保留为 source_kind） | 含义 | dna_class |
|---|---|---|
| profile | 身份类（identity） | dna |
| preference | 偏好类（preference） | dna |
| instruction | 程序性要求（procedure） | dna |
| project | 项目上下文 | standard |
| tool | 工具上下文 | standard |
| 未知或缺失 | 不按正文猜类别 | standard |

未知分类的 `evidence_level` 按既定计划标为 `inferred`，原类别不丢；
分类明确的网页导出按第三方核对标为 `third_party`，不冒充真导出实测。
读取已有 OKF 家时保留原有分类与证据等级，不重新猜测。
Claude Code 的 `metadata.type=feedback` 不擅自升级成 DNA，列为未知分类，
待用户给出额外规则。映射表将随计划报告显式列出。

路径比较：

1. **采用既定表（推荐）**：不猜正文，只保护显式身份、偏好与指令类别，规则可核对。
2. **全部标为 DNA**：不会漏保护，但项目和工具背景也会阻塞目标映射，偏离当前默认表。
3. **全部标为 standard**：实现最少，但削弱设计中对身份与指令的保护，不推荐。

## 2. 未知字段必须有真正的传递通道

现有 Reader 能交出源字段和 unmapped，但引擎传给 Writer 的只有 canonical。
因此“源里保留了未知 frontmatter”不等于“写回目标时还能保留”，M4 需要补这条通道。

路径比较：

1. **内部 canonical 加可选 source_extra（推荐）**：
   只装本条记录无法语义归一的源 metadata，不装全文、整个导出文件或 canonical 快照。
   字段仍进入 unmapped 清单，同时说明其保留位置；它不是独立格式或归档。
   OKF 放在现有 mem_adaptor 扩展内，读回恢复，现有 record_hash 自动绑定；
   密钥闸门同样扫描，避免传递通道绕过检测。目标无法保存时必须显式报告，
   DNA 类字段不静默丢弃。
2. **另外给 Writer 增加源字段上下文参数**：
   canonical 不新增字段，但 plan/write/read_back 的接口与摘要都要增加单独的绑定，
   家目录往返时还要带回上下文。接口改动更大，不推荐用于本轮。
3. **继续只报告未承载、不保留值**：
   没有接口改动，但只能降级交付，不能满足 M4 的未知字段写回要求。

采用推荐路径后，更新内部 canonical schema、Rust 类型和合成向量；
不将 source_extra 或 canonical 对象写进计划/回执报告。
多条记录文件的字段映射按记录关联，不能让一条记录的映射掩盖另一条的漏报。

## 3. 实现切片

1. **Markdown/OKF**：保留正文字节；已知 frontmatter 明确映射，未知 metadata 保留；
   MEMORY.md 只登记；Claude Code 会话来源与时间不混成事实时间；
   OKF 家恢复原身份、scope、分类与 metadata；空目录明确报空源。
2. **ChatGPT**：saved memories 两种 JSON 外壳；删除条目只计数；
   Prompt 三列文本坏行进 anomalies，未知类别保留，不补造日期时刻；
   conversations.json 和 user.json 只登记，显式报告合成层及 provenance 的不可得。
3. **Claude**：memory_files 存在即优先，包括空数组；旧字段不重复导入；
   没有该字段才读旧版整块账号记忆和逐项目记忆；
   projects.docs 与非空 prompt_template 逐对象导入；
   conversations.json 只登记并计数。
4. **CLI 与验收**：注册三个 Reader，明确区分导出形状，避免同名文件重复认领；
   只用合成目录/ZIP fixtures 和脱敏、稳定化的报告快照；
   验证家目录往返、正文保留、字段覆盖、坏行继续、删除计数和空源；
   再跑全部 workspace 验证。

## 4. 后续依赖

M5 的完整 Writer 依赖上述字段传递方式；其 kind/title 映射按原计划另行确认。
M6 在 Reader/Writer 稳定后接家模式与配置。
M7 的真导出验收依赖用户提供或明确允许的真实数据，目前不读取任何真实记忆。

## 5. 已落地的输入与报告约定

- Prompt 文件使用 `*.chatgpt.md`，避免把 Gemini 的同形文本错归 ChatGPT；
  日期保留在 `source_extra.date`，不是事实时刻。重复的归一化行只保留一条并报异常。
- saved-memory JSON 接受 `memory.json`、`saved_memories.json`、`memories.chatgpt.json`；
  `memories.json` 根据对象/条目形状与 Claude 的账号数组区分。
  原始字符串 id 原样保留；无 id 时采用正文哈希，内容变化会形成新记录。
- Claude 新版记录以账号和原始文件路径组成稳定源标识；完整 `content` 字节不改，
  含 Markdown frontmatter 时只读取显式类别，不按 `/profile.md` 等文件名猜类别。
  项目文档优先保留原始 uuid，无 uuid 则按项目和文件名/正文哈希定位。
- 分类/保留规则列在 `field_map.rule`；未知类别保留原值并列 `unmapped`。
  未归一 metadata 同时列 `unmapped` 与 `/source_extra/...` 保留位置。
  原样字段与这些清单在内部按 `canonical_id` 关联，不同来源的同名原始 id 不能互相覆盖。
- 根 `index.md` 带 OKF Index frontmatter 才连同 `log.md` 视为家索引/日志；
  `.mem-adaptor/` 下的运行产物不当记忆。正文提到 `okf_version` 不算家声明。
- 空会话数组需同目录的来源证据；孤立 `conversations.json=[]` 不猜来源，列为未认领。
  已认领文件的错误 JSON/YAML 或外壳类型失败；坏条目/Prompt 坏行报告路径或行号，不复制原值。
  saved memory 同时给不同 `content` 与 `text` 时报告冲突，不自动选择全文。

**验收产物**：

- `crates/cli/tests/readers.rs`：16 项合成 Reader 验收。
- `crates/cli/tests/fixtures/m4/`：三个来源的输入与三个完整计划快照，全部合成。
- 核心与引擎另外固定了逐条字段覆盖、跨来源原始 id 重名的回归。
- 家往返比较完整 `record_hash`，不只比正文；ZIP 同样走审批后的真实临时写入。
- 原源 metadata 与家信封的同名叶子值冲突时失败，不自动覆盖；无冲突新增信封字段保留。
- 快照固定运行 id、时刻与源/目标路径，然后重算摘要；已通读源清单、规则、未承载字段、
  不可得、异常和处置。快照不用于 `apply`，也不替代 M7 的独立 conformance。
