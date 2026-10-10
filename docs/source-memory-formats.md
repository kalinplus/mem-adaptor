# 源侧记忆形态：网页端 + Harness

调研对象分两类，都是 Reader 的潜在源：

- **网页端**（服务器侧记忆）：ChatGPT、Claude.ai、Gemini。
- **Harness**（本地 agent 工具的记忆）：Claude Code、Codex CLI、Gemini CLI，以及 AGENTS.md 家族（本机 pi 实测）。

证据分级：**实测** = 本机落盘直接打开看的；**文档** = 官方文档/源码注释；**传闻** = 第三方页面，口径不一致，进「待核实」。

## 结论先行

1. **网页端分两类，不是一类**：只有 Claude 能把记忆放进导出包（`memories.json`），
   ChatGPT 与 Gemini 的记忆**结构上就导不出来**——前者是服务端合成物、后者是从活动记录归纳的
   派生层。所以 Reader 也要分两类：**导出包解析器**（Claude）与 **Prompt 抽取器**
   （ChatGPT / Gemini，见 [reader-prompts.md](reader-prompts.md)）。迁移报告必须显式列
   「源侧不可得」，这是铁律 6（禁止静默衰减）在 Reader 侧的对应物。
2. **Harness 侧全部是本地文件，已收敛成三种形态**：人写的指令 markdown（AGENTS.md / CLAUDE.md / GEMINI.md）、agent 自写的 memory 目录（Claude Code 与 Codex 都叫 `MEMORY.md`）、append-only 会话 jsonl。三种都能直读，是 D1 Reader 里性价比最高的一档。
3. **`MEMORY.md` 已是事实标准名字，但同名不同语义**：Claude Code 是「全量截断注入的索引」，Codex 是「必须关键词检索并强制引用行号范围的注册表」。迁移时不能按文件名对等。
4. **本地落盘 ≠ 不出网**：harness 的 memory 内容每轮随 prompt 出站；Codex 的 secret redaction 发生在模型已经看过原文之后。

## 本机实测证据（2026-09-30）

```
~/.claude/projects/<slug>/memory/      Claude Code 自动记忆，10 个项目有目录
~/.claude/projects/<slug>/*.jsonl      会话 transcript（96 个文件）
~/.claude/history.jsonl                用户 prompt 历史
~/.codex/memories/                     空目录（功能在、无数据）
~/.codex/memories_1.sqlite             stage1_outputs 表 0 行
~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl   会话 transcript（19 个文件）
~/.codex/state_5.sqlite                threads 表，含 memory_mode 列
```

## 五问对照：网页端

| 五问 | ChatGPT | Claude.ai | Gemini |
| --- | --- | --- | --- |
| 1 接线/触发 | 模型自觉（可说 remember this）；两层：saved memories 条目 + reference chat history（隐式召回历史对话）。2026-06 起改为 memory summary 合成，自动更新 | 模型自觉 + 用户明说；**按对话实时增量存 topic**，不是会话结束再汇总；每个 Project 有独立记忆空间和 project summary | Saved info（显式条目）+ 隐式个性化；Gems 各自带 instructions；Personal Intelligence / Connected Apps 把 Search/YouTube/Gmail/Drive/联系人的数据并进来 |
| 2 存储/真相 | 服务器侧，账号级；本地无落盘。真相只能通过导出拿 | 服务器侧；导出是唯一通道 | 服务器侧；Takeout 只覆盖 activity，Saved info 不在内 |
| 3 变更 | 自动 + 手动；2026-06 起由系统改写合成，可回退旧 saved memories 列表 | 随时增量；删会话**不会**删由它产生的 memory 条目；可 pause / reset | 手动或自动；Keep activity 关闭后对话仍保留 72h |
| 4 检索 | 服务端不透明；reference chat history 是隐式召回 | 服务端；另有独立的 chat search（搜历史对话）开关 | 服务端；隐式个性化 |
| 5 边界 | 全部在 OpenAI 侧；导出是唯一出站通道 | 全部在 Anthropic 侧；**唯一有源侧闸门的**：敏感话题默认不记，需显式开启 | 全部在 Google 侧；**边界最大**：Connected Apps 把 Gmail/Drive/YouTube/位置一并纳入 |

### 导出产物与缺口

| 系统 | 导出路径 | 文件 | 缺口 |
| --- | --- | --- | --- |
| ChatGPT | Settings → Data Controls → Export data（邮件链接 24h 有效，最长 7 天） | `conversations.json`（mapping 树 + `current_node`）、`chat.html`、`user.json`、`message_feedback.json`、`model_comparisons.json`、`shared_conversations.json`，记忆见下 | **无 per-entry provenance**（没有 `source_conversation_id`）；合成层（memory summary / reference chat history）没有独立导出；删除的条目不在导出，`enabled:false` 的保留；Team/Enterprise 的 workspace 导出不含成员记忆（记忆是账号级） |
| ChatGPT 记忆文件 | 同上 | **不在导出包里**（第三方对真实导出包的核对结论：自定义指令包含、记忆不包含） | 记忆是「持续更新的合成物」，不是可落盘文件；官方对导出的措辞只有含糊的「聊天记录和其他相关账号数据」。→ 只能走 Prompt 抽取，见 [reader-prompts.md](reader-prompts.md)。仍**待真导出核实** |
| Claude.ai | Settings → Privacy / Data → Export Data（ZIP） | `conversations.json`、`projects.json`、`memories.json`、`users.json` | 官方 FAQ 明说「All memory data is included in data exports」。`memories.json` = **单元素数组**：新版 `memory_files[]{path,content,updated_at}` + 旧版 `conversations_memory`/`project_memories`/`account_uuid`（详见 [PROGRESS.md](PROGRESS.md) 的「网页端记忆导出形态」一节）。`conversations.json` 里有 `project_uuid` 但没有 project 名字 |
| Gemini | takeout.google.com：勾 `Gemini`（Gems）+ `My Activity` → 只勾 `Gemini Apps` | `My Activity.json`（Gemini Apps 段，走 Google Data Portability 的 My Activity schema） | **只有用户 prompt，没有任何模型回复**；记忆**结构上就导不出**——它是从活动记录检索归纳出的派生层，官方文档里没有任何记忆/个性化导出选项 |

### 会话导出格式的取数规则（remnic 的 importer 已给出可复用做法）

- ChatGPT conversation：沿 `current_node` → parent 链只走活跃分支，丢弃被放弃的分支；`--include-conversations` 时每会话压成一条「用户轮摘要」。
- Claude conversation：只取 human 轮，assistant 回复丢弃。
- Claude project：`docs[].content` 1:1 成一条，`metadata.kind=project_doc`；prompt template 非空则一条 `project_prompt_template`。
- Gemini：每条 prompt 一条，短于 10 字符的默认丢（过滤「好的」这类），含 pre-rebrand 的 Bard 记录。

## 五问对照：Harness

| 五问 | Claude Code | Codex CLI | Gemini CLI |
| --- | --- | --- | --- |
| 1 接线 | 两套并行：`CLAUDE.md` 四 scope（人写）+ auto memory（模型写，v2.1.59+ 默认开）+ 会话 jsonl | `AGENTS.md` 链（人写）+ memories 两阶段 pipeline（启动时后台跑，**默认关闭**；细节见 [codex-memory.md](codex-memory.md)）+ rollout jsonl | `GEMINI.md` 层级（人写）+ `save_memory` 工具（模型自觉）+ 会话历史 |
| 2 存储/真相 | `~/.claude/projects/<slug>/memory/`：`MEMORY.md` 索引 + topic .md；transcript 在 `~/.claude/projects/<slug>/<session-uuid>.jsonl` | `~/.codex/memories/`：`MEMORY.md` + `memory_summary.md` + `raw_memories.md` + `rollout_summaries/` + `skills/` + `extensions/`；中间态在 `~/.codex/memories_1.sqlite` 的 `stage1_outputs` 表 | 纯 markdown 文件：全局 `~/.gemini/GEMINI.md`（`save_memory` append 到「Gemini Added Memories」段）+ 项目/子目录 `GEMINI.md` |
| 3 变更 | 模型自己决定写什么；`MEMORY.md` 逼近 200 行/25KB 时 harness 提醒压缩、超限则报错要求重写 | Phase 1 逐 rollout 抽取（生成后做 secret redaction）→ Phase 2 用 git diff 决定要不要跑 consolidation agent，按 `usage_count`/`last_usage` 选 top-N，裁剪 stale 的 rollout summary | 模型调 `save_memory` append；人可手改，`/memory refresh` 重载 |
| 4 检索 | 启动注入 `MEMORY.md` 前 200 行或 25KB，topic 文件靠模型主动 read；**无向量检索**；`--resume` 只搜标题/时间，全文要自己 grep | `MEMORY.md` 关键词搜 → 按需开 rollout summary → 必要时扫 `rollout_path`；**强制要求引用**（`:<start>-<end>\|note=[]` + `rollout_ids`） | 无检索：全部 concatenate 进每个 prompt，靠上下文预算截断 |
| 5 边界 | 全本地文件；但内容是每轮 prompt 的一部分 → 出网 | 全本地文件；Phase 1/2 都发模型，**redaction 在模型看过原文之后** | 全本地文件；随 prompt 出网 |

### 身份字段（跨系统对不上的那一列）

| 系统 | 身份 | 说明 |
| --- | --- | --- |
| ChatGPT / Claude.ai / Gemini | 账号 | 无 user 字段，账号即 scope |
| Claude Code | git 仓库 | `<slug>` = cwd 的 `/` 换 `-`（实测 `-Users-kalin-github-mem-adaptor`）；同一 repo 的 worktree 与子目录共享一个 memory 目录；machine-local，不跨机器 |
| Codex | thread | `session_meta.payload.id` = thread id，rollout 文件名里带这个 uuid；`threads` 表存 `rollout_path`/`cwd`/`title`/`memory_mode` |
| Gemini CLI | 文件层级 | global / project / subdir 三级，没有更细的身份 |

### 实测落盘细节

**Claude Code auto memory**（实测 10 个项目目录，`MEMORY.md` 都是 markdown 链接索引）：

```markdown
# Memory Index

- [oh-dsh commit 不加 AI 署名](ohdsh-no-ai-attribution.md) — 无 Assisted-by trailer、body 简洁；派发 prompt 要显式禁止
```

topic 文件的 frontmatter 有两种变体（同一台机器上都实测过）：嵌套式（`metadata.` 块，`type` 实测见过 `project`、`feedback`、`user`）与**扁平式**（顶层 `name`/`description`/`type`，无 `metadata:` 块，`type` 实测见过 `reference`、`feedback`、`project`；有的目录整目录都是这种）：

```yaml
---
name: agent-resume-entry-toggles
description: agent.tex 中秒填鸭、个人 AI-Native 工作流两个条目的取舍决策
metadata:
  node_type: memory
  type: project
  originSessionId: 104ba331-a925-4fd1-9f9c-a2c3d6d65869
  modified: 2026-08-25T15:34:22.507Z
---
```

- `modified` 由 harness 在写入时补，只对带 frontmatter 的文件生效 —— 所以有的文件没有这个字段。
- `originSessionId` 是唯一能指回会话的 provenance 字段，但对应的 jsonl 可能已被清理。
- 已知 bug 面：`autoMemoryDirectory` 在部分 settings scope 下不生效；系统提示词里的路径说明与实际加载路径不一致（GitHub issues #36973 / #42682 / #46701）。

**实测迁移（2026-10-10，Issue #44）**：四个目录（含整目录扁平式 frontmatter 的变体）经 markdown Reader 进临时 OKF 家全链路验收，32 条记录：

- 正文逐字节一致；`name`/`description` 经 `source_extra` 完整保留进 OKF 的 `mem_adaptor:` 扩展块，回读可复原；`metadata.modified` → `sources[0].last_modified` 逐条一致，没有该字段的文件（扁平式或缺这个键）不捏造时间。
- `MEMORY.md` 被 Reader 登记（哈希、字节数）但**不作为记录迁移**（Claude Code 的索引是从 topic 文件派生的展示层，迁了就重复）；报告里状态是 `registered_only`、`layer: index`，不是静默跳过。
- 两种变体的 `type` 都落 `source_kind`，但源指针不同：嵌套式读 `/frontmatter/metadata/type`（且需 `metadata.node_type: memory` 才认作 Claude Code；否则该 `type` 不落 `source_kind`，只留在 `source_extra`/`unmapped`），扁平式读 `/frontmatter/type`，两者 canonical 目标都是 `/source_kind`。分类三档：`project`/`tool` 落 `explicit_standard`，`profile`/`preference`/`instruction` 落 DNA 桶，其余（如 `feedback`、`user`）落 `unknown_standard`：`evidence_level: inferred` 并列入 `unmapped`（语义未解释）。完全无 `type` 的文件不设 `source_kind`（`evidence_level` 同样落 `inferred`）——不按文件名或正文猜类别（[reader.rs](../crates/core/src/reader.rs) 的 classify 契约）。
- OKF `title` 规则会剥掉正文首行的 markdown 标题标记（`#`/`##`）再取 80 个字符；字段保留清单（unmapped）标记的是**语义未解释**，与字节数保留（`source_extra`）是两个维度，报告同时给出两者。
- 同一目标背靠背生成的多个计划，先 apply 的那个会推进共享产物（index/log）依据，其余计划的 apply 因依据漂移被拒（退 4），需重新出计划——多卫星依次汇入时的预期行为，见 [m6-cli-proposal.md](m6-cli-proposal.md) §1。

**Claude Code 会话 jsonl**（实测事件类型，`parentUuid` 构成消息树）：

```
user / assistant / system / attachment / file-history-snapshot / file-history-delta
/permission-mode / mode / last-prompt / atis-latch / cost-state
```

`attachment` 类型里含 hook 的完整 stdout（本机实测塞进了 SessionStart hook 的整段 additionalContext），`file-history-*` 是文件备份引用（实体在 `~/.claude/file-history/<session>/`）。另有大 transcript 之外的目录：`~/.claude/tasks/<session>/`、`~/.claude/projects/<slug>/<session>/subagents/agent-*.jsonl`。

**Codex memories pipeline**（源码文档 `codex-rs/memories/README.md`；官方口径、默认值与边界见 [codex-memory.md](codex-memory.md)，下面这批文件布局属**源码实现细节、非对外契约**）：

- 触发条件：会话非 ephemeral、memory 开启、非 sub-agent、state DB 可用；启动时后台异步跑。
- Phase 1（按 rollout 并行）：挑「来源合法 + 在年龄窗口内 + 闲置够久 + 没被别的 worker 占用」的 rollout → 只留 memory 相关 response item → 发模型 → 收 `raw_memory` / `rollout_summary` / `rollout_slug` → 做 secret redaction → 落 `stage1_outputs` 表；失败带 retry backoff。
- Phase 2（全局单锁）：选 top-N（`usage_count` 优先，`max_unused_days` 之外的丢弃）→ 同步 `raw_memories.md`（thread-id 升序，避免排序抖动）与 `rollout_summaries/` → **整个 memories 目录是 git baseline（`.git`）**，用 git diff 判断有无变化 → 有变化则写 `phase2_workspace_diff.md` 并起 consolidation sub-agent（无网、无审批、只能本地写、禁 collab，防递归委派）→ 成功后重置 baseline。
- 这套设计的直接借鉴价值：**用 git 工作区而非 DB watermark 当 dirty check**；**用 usage 计数做记忆衰减**；**强制引用源行号范围**。
- 实测本机：cli 0.142.5，`memories/` 为空、`stage1_outputs` 0 行 —— 功能在但从未产出。「有代码不等于有数据」是 Reader 的常态输入。

**Codex rollout jsonl**（实测事件类型）：

```
session_meta    → id / cwd / cli_version / model_provider / base_instructions(全文)
turn_context    → model / cwd / current_date / approval_policy / user_instructions(实际加载的 AGENTS.md 全文)
event_msg       → task_started 等轻量状态流
response_item   → 真实消息、工具调用与工具输出
```

`turn_context.payload.user_instructions` 是现成的**指令记忆 provenance**：转录里直接记下了那一轮到底加载了哪份 AGENTS.md 文本。

**Gemini CLI**（文档 + 实测）：`@file.md` 导入支持相对/绝对路径，带循环导入检测、最大深度 5、路径白名单；`/memory show` 看当前加载了哪些文件、`/memory refresh` 手动改文件后重载。本机 `~/.gemini/GEMINI.md` 与另三家 md5 相同，说明 `save_memory` 从未写过「Gemini Added Memories」段。

**pi / AGENTS.md 家族**（实测）：`~/.pi/agent/memory/rule-*.md`，结构是「来源 / 结论 / 行为 / 关联」四段；会话在 `~/.pi/agent/sessions/<cwd-slug>/<ts>_<uuid>.jsonl`，v3 schema，事件类型 `session` / `model_change` / `thinking_level_change` / `message`。

## 横向规律

1. **落盘形态收敛到三种**，harness 侧无一例外全是文件，网页端无一例外全是「导出」。这条决定了架构：harness Reader 是文件解析器，网页端 Reader 是导出格式解析器加一份「缺项清单」。
2. **同名不同语义**：`MEMORY.md` 在 Claude Code 是索引（全量注入、截断），在 Codex 是注册表（检索、引用、衰减）。迁移不能按文件名对等，必须按加载语义对等。
3. **身份维度天然不兼容**：账号 / git 仓库 / thread / 文件层级四种，没有任何一家存 user 字段。scope 只能由迁移管道赋，不能从源里继承。
4. **时间语义三档**：Claude Code 的 `modified` 是 harness 写入时间（≠ 事实时间，和 basic-memory 同一个坑）；Codex 有 `source_updated_at` / `generated_at` / `last_usage` 三个时间；网页端条目基本只有 created_at。
5. **PII 现状**：harness 侧无拦截且内容本来就要出网；Codex 的 redaction 在抽取之后；Claude.ai 是唯一有源侧闸门（敏感话题默认不记）；Gemini 因为 Connected Apps 边界最大。「源系统不设卡」再次成立。
6. **检索方式决定写入要求**：Claude Code 是「注入即用」，所以内容要短；Codex 是「检索 + 引用」，所以内容要能指回源；Gemini CLI 是「全量拼进去」，所以内容直接吃上下文预算。Writer 要按目标检索方式调形状。

## 对本仓库的直接结论

**D1 Reader 优先级（按成本/收益）**

1. Claude Code：`projects/<slug>/memory/`（markdown + frontmatter，直读）+ `<slug>/*.jsonl`（只取 user/assistant 文本 + 时间 + session id）。
2. Codex：`memories/` 目录（若为空则报告空源）+ `sessions/**.jsonl`（事件流里挑 `response_item`，`turn_context` 取指令 provenance）。
3. 通用指令文件：`AGENTS.md` / `CLAUDE.md` / `GEMINI.md` / `.claude/rules/*.md` / `.claude/commands/*.md`，一个 glob 覆盖。
4. 网页端：**Claude 导出包解析器**（`memories.json` 新版 `memory_files` 优先、旧版字段兼容 + `projects` + `conversations`）；**ChatGPT / Gemini 走 Prompt 抽取器**（[reader-prompts.md](reader-prompts.md)），`conversations.json` / `MyActivity.json` 只当原始素材。

**Writer 侧新目标**：Claude Code 的 `memory/` 目录、Codex 的 `memories/` 目录、AGENTS.md 家族。这三个比 UMP/OKF 更「有人真的在用」，但都是私有约定，写入前必须 dry-run。

**迁移报告 schema 必带字段**

| 字段 | 取值来源 |
| --- | --- |
| `source_system` | chatgpt / claude_ai / gemini / claude_code / codex / gemini_cli / agents_md |
| `source_layer` | instruction / auto_memory / transcript / project_doc |
| `identity` | account / repo / thread / path（附原值，如 slug、thread id） |
| `source_timestamps` | created / modified / source_updated_at / last_usage |
| `provenance` | `originSessionId` / `thread_id` / 文件行号范围 / **无**（ChatGPT 就是无） |
| `egress` | 该条内容在源侧是否已经发过远程模型 |
| `source_unavailable` | 源侧不可得清单（合成层、Saved info、project instructions 字节…） |

**可复用的两个现成设计**

- Codex 的强制引用（每条迁移结果能指回源文件的行号范围）——正是「机器可验证凭证」在 Reader 侧的形状。
- Codex 的 git baseline + workspace diff 当 dirty check——比 DB watermark 更适合我们要产出 diff 型迁移报告的形态。

**现成的不变式校验用例**：本机 `mem-adaptor` 的 Claude Code memory 目录为空、Codex memories 为空。Reader 必须能处理空源并在报告里说清「功能存在但无数据」，而不是报错或静默产出空报告。

## 待核实清单

| # | 问题 | 现状（2026-10-04） | 怎么核实 |
| --- | --- | --- | --- |
| 1 | ChatGPT 的记忆到底在不在导出包里 | 第三方对真实导出包的核对结论是**不在**（自定义指令包含、记忆不包含）；官方措辞含糊。**按「不在」设计**，所以配了 Prompt 兜底 | 真跑一次 Settings → Data Controls → Export data，打开 ZIP 确认有没有记忆文件（help.openai.com 直连 403） |
| 2 | Claude.ai `memories.json` 的真实结构 | 已定型：**单元素数组**，新版 `memory_files[]{path,content,updated_at}` + 旧版 `conversations_memory`/`project_memories`/`account_uuid`。但**具体内容形态仍是第三方口径**（`path` 命名规律、`content` 有没有 frontmatter 说法不一） | 真跑一次 Claude.ai Export Data，重点看 `memory_files` 的实际 `path` 与 `content` 形状 |
| 3 | Gemini 记忆是否完全不可导出 | 结论**确认**：Takeout 只有「Gemini」（Gems 配置）和「我的活动记录 → Gemini 应用」（对话/媒体/上传）两个入口，官方文档无任何记忆/个性化导出选项；记忆是从活动记录归纳的派生层 | 实测一次 Takeout，确认 `My Activity.json` 里有没有任何记忆段（预期：无） |
| 4 | Claude.ai 的 project instructions 文本与 knowledge 文件字节是否在导出里 | 仍未定；`projects.json` 的 `docs[]` 含 `content`，但 `memories.json` 的 `project_memories` 与它是否重叠、project instructions 落哪个字段不清楚 | 用一份真导出对照 `projects.json` 与 `memories.json` 的 `project_memories` |
| 5 | Codex memories pipeline 的真实产物格式与官方默认值 | 已展开到 [codex-memory.md](codex-memory.md) 的待核实清单（需先打开 `[features] memories`） | 同 codex-memory.md |
