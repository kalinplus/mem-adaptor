# 记忆产品形态总览：我们实验过的八套系统

**这份文档解决什么问题**：Step 0–5b 一共摸过八套系统，每套的落盘形态、身份字段、冲突处理都
不一样。详细记录散在 [PROGRESS.md](archive/PROGRESS.md)（逐步骤的实验日志）和
[step4-5-report.md](archive/step4-5-report.md)（逐字段矩阵）里，但**没有一份「一眼看懂八套分别是什么形态」
的总览**。这份文档就是那份总览。

**怎么读**：第 1 节给你一个判断框架（记忆到底存在哪），第 2 节是全表速查，第 3 节逐个讲清机制，
第 4 节是横向规律。想知道某一条记忆的**具体字段**，去 [step4-5-report.md](archive/step4-5-report.md)。

**这份文档讲的是「目标侧」**——也就是我们试着往里写的那批系统。从真实平台往外读的「源侧」
（ChatGPT / Claude / Gemini 导出、Claude Code / Codex 本地记忆）在
[source-memory-formats.md](source-memory-formats.md) 和 [codex-memory.md](codex-memory.md)。

---

## 0. 先对名字

这几个名字最容易混，先钉住（完整身份与地址见 [ecosystem.md](ecosystem.md)）：

| 口头说法 | 正式名 | 是哪个 | 一句话 |
| --- | --- | --- | --- |
| CloudMem / CMEM | **claude-mem** | Step 4 | Claude Code 的旁路钩子记忆，安装时会问你要不要登录它的云端账号 |
| — | **agentmemory** | Step 4b | 同类的钩子记忆，但**默认零 LLM、不出网** |
| 「Memorine」 | **mem0** | Step 2 | 把对话抽成事实的主流记忆库（实验里喂过它，也在迁移走查里当**源**样本） |
| 「Memorine」 | **Memoria** | Step 5 | 带快照/分支/回滚的记忆（注意：和 mem0 只差几个字母） |
| — | **Panella** | Step 5b | 写入要人批准的治理箱子 |

> ⚠️ mem0 与 Memoria **不是同一个东西**，名字只差几个字母，本仓库的文档里务必写全。

---

## 1. 判断框架：记忆存在哪？

这是理解所有差异的枢纽。问一个问题：**那套系统的记忆，是不是一个你能直接打开的文件？**

| 类别 | 记忆存在哪 | 读它要什么 | 本批属于这类的 |
| --- | --- | --- | --- |
| **文件式** | 磁盘上的文件 | `cat` 就能看 | server-memory（jsonl 裸图）、basic-memory、agentmemory |
| **数据库式** | 数据库里 | 会连库、会查表 | mem0、claude-mem、Memoria、Panella |
| **图谱式** | 图数据库 | 会 Cypher 查询 | Graphiti |

**注意：这八套没有一套是「算出来的」。** 它们都是真正落盘的东西——这一点和
ChatGPT / Gemini 的记忆有本质区别（那两家的记忆拿不走，只能问出来）。
**只要你会读库，这八套的记忆就完全在你手里。**

「读的难度」是有实际代价的：文件式你随时能 `git diff`、人读人改；数据库式你得先知道表结构
和连接方式；图谱式还要会图查询语言。这个代价直接决定迁移工具的实现成本。

---

## 2. 全表速查

| Step | 系统（版本） | 形态 | 载体 | 身份字段 | 内容是否原文 | 默认出网 | 冲突怎么办 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | server-memory 0.6.3 | 裸图谱 | `memory.jsonl` | **无** | ✅ 原文 | ❌ 全本地 | 并存 |
| 1 | basic-memory 0.22.1 | Markdown+frontmatter | `.md` 文件（+ 可再生 SQLite 索引） | `permalink`（无用户） | ✅ 原文 | ❌ 全本地（首跑下模型） | 并存 |
| 2 | mem0 OSS 2.2.1 | 事实+向量+KV | Qdrant sqlite（**pickle**）+ `history.db` | `user_id` + `run_id` | ❌ LLM 转述 | ✅ 抽取+embedding | 并存（LLM 判 ADD/UPDATE，实测误判） |
| 3 | Graphiti 0.30.2 | 双时序知识图谱 | Neo4j（**无文件**） | `group_id` | ❌ LLM 转述 | ✅ 每集多次 | **标 invalid_at**（唯一进语义层） |
| 4 | claude-mem 13.29.0 | 旁路钩子 | SQLite 31 表 + Chroma(384) | `project`（无用户） | ❌ 二次转述 | ✅ **每次工具调用** | 并存 |
| 4b | agentmemory 0.9.29 | 旁路钩子 | `.bin` 文件（JSON+二进制尾） | `sessionId` | ✅ 原文 | ❌ **零 LLM（BM25）** | 并存 |
| 5 | Memoria 0.5.2 | 常驻服务 | MatrixOne（**无文件**，按用户分库） | `user_id`（分库） | ✅ 原文 | ✅ 按 `.env` | 并存 + **回滚** |
| 5b | Panella 0.2.1 | 常驻服务 | SQLite + sqlite-vec | `tenant_id`/`wing`/`room` | ✅ 原文 | ✅ 按配置 | 并存 + **人批准** |

**这张表最该注意的两列**：

- **「身份字段」没有两家能对上**——从 `无` 到 `permalink`、`user_id`、`group_id`、`project`、
  `sessionId`、三段式，八套八个答案。迁移时「这条属于谁」只能由管道自己赋值。
- **「默认出网」一半是一半不是**——而且出不出网和记忆质量**不是一回事**（见第 4 节规律三）。

---

## 3. 逐个讲：它们的机制到底是什么

### Step 0：server-memory —— 记忆的最小可能形态

官方出的最小参考实现，也是我们整条实验线的基线。

**形态**：一个 `memory.jsonl` 文件，每行一个 JSON 对象，两种类型：

```jsonl
{"type":"entity","name":"Kal","entityType":"user","observations":["Uses Obsidian for personal notes (R3)","API key is sk-test-12345, remember for next time (R4)"]}
{"type":"entity","name":"project-db","entityType":"component","observations":["Migrated from MongoDB to PostgreSQL because transaction requirements got higher (R1)","Migrated back to MongoDB because PostgreSQL ops was too heavy (R5)"]}
```

**它的「最小」体现在哪**：四个字段（`type`/`name`/`entityType`/`observations`），
**没有身份字段、没有时间戳、没有 schema 版本、没有 provenance**。

**两个机制上的坑**：

1. **抽取是调用方的活**——server 里没有任何大模型，9 个工具全是 CRUD，对话怎么拆成
   实体/观察值**由 client 侧的 LLM 决定**，同一句每次拆得可以不同。
2. **检索是纯子串匹配**——`searchNodes` 就是实体三字段的 `toLowerCase().includes()`。
   实测里 GLM 搜「数据库」扑空（observation 里没这三个字），换搜 `project` 才命中。
   **字面检索的语义鸿沟，靠 LLM 重试来救。**

**为什么先讲它**：后面每一套多出来的字段，都对应一个它没解决的问题。它是标尺。

### Step 1：basic-memory —— 文件即真相

**形态**：Markdown 文件 + frontmatter（文件开头 `---` 包起来的那段元数据）。关系写在正文里，
用 `[[wiki链接]]` 表示，索引时被解析进 relation 表。

```markdown
---
type: person
permalink: memlab/kal
tags:
- user-profile
---

# Kal

- Uses Obsidian for personal notes (R3)
- Favorite editor is Neovim (R3)
```

**它是 A 类里对迁移最友好的形态**，理由有两条：**文件即真相**（`~/.basic-memory/memory.db`
只是派生索引，删了能 `reset --reindex` 重建）+ **可 git diff、人读人改**。
它的治理思路是「索引坏了就重建」，不是「文件服从数据库」——**它把你当文件的所有者、
把自己当索引的租客**。

**身份字段是 `permalink`**（如 `memlab/kal`），**没有用户维度**。

**两个坑**：① `write_note` 对已存在笔记**拒绝覆盖**，但 `edit_note` 的 append
**重复调用就重复追加**（无幂等）；② 检索用本地 `bge-small`，那是**英文模型**，
中文 query 会命中错误的笔记。

### Step 2：mem0 —— 「一句话 + 一个向量」

主流记忆库的开源版。**形态和前面几套完全不同：内容不再是原文。**

**载体**：Qdrant 本地文件。**注意格式是 Python 的 pickle**，不能直接读，得先解开：

```json
{
  "id": "397fc002-...",
  "vector": { "": [ 2048 个浮点数 ] },
  "payload": {
    "user_id": "kal", "run_id": "memlab-run1",
    "data": "User's project recently switched from MongoDB to PostgreSQL around September 2026 due to increased transaction requirements.",
    "text_lemmatized": "...", "hash": "4b2cd412...",
    "created_at": "...", "updated_at": "...", "attributed_to": "user"
  }
}
```

**最关键的一点**：我们喂进去的是中文原句「我们项目最近把 MongoDB 换成了 PostgreSQL」，
它存下来的是**英文的第三方转述**。**原始对话已经被丢掉了**——这是一次不可逆的有损转换。

**身份字段是 `user_id`**，是八套里第一个真正有「用户」维度的。

**它怎么处理矛盾**（招牌功能，但实测没触发）：它让 LLM 判断每条新记忆该
ADD / UPDATE / DELETE。我们喂矛盾句 R5（「又迁回 MongoDB 了」），**它判成了 ADD 而不是
UPDATE**，于是两条**并存**。

**一个必须警惕的细节**：那句 `data` 里的 "around September 2026" **是我们没喂过的信息**——
它来自抽取 prompt 里的当前时间，被模型织进了事实。**抽取产物 ≠ 源文本的子集**，
provenance 必须区分「源文本说的」和「抽取器补的」。

**还有一份变更日志**（`history.db`）：记录每次操作是 ADD/UPDATE/DELETE、旧值新值。
**要看懂「记忆怎么变的」，光看主库不够。**

### Step 3：Graphiti —— 双时序知识图谱

**形态**：Neo4j 图数据库，**宿主机上没有任何人类可读文件**。存的是三元组
`实体 -[关系]-> 实体`，比如 `用户 -[切换到]-> PostgreSQL`。

**它最特别的地方：每条边有三组时间**，这是它和别家最大的字段差异：

| 字段 | 含义 |
| --- | --- |
| `valid_at` | 事实**从何时开始**成立 |
| `invalid_at` | 从何时**开始不成立** |
| `expired_at` | 系统**何时真的把它标记了** |
| `created_at` | 这条边**何时写进库** |

前两个是「**事实时间线**」，后两个是「**系统时间线**」——**「双时序」指的是这两组，
不是四个同义词**。

**它怎么处理矛盾**：喂 R5 后，它把 R1 的三条边**全部标上 `invalid_at`**，值正好等于 R5 的
`valid_at`。**这是八套里唯一把矛盾写进语义层的。**

**但「标失效」不等于「裁决」**：它没说哪条对，只是把「何时开始不真」记下来，
**选择权还在读的人（或下游 LLM）手里**。

**两个实测到的怪事**：
1. **去重只做失效、不做合并**——R1 的三条同义转述边（「迁移走」/「切换为」/「被替换」）
   没被并成一条，是三条并存、一起被标失效。
2. **失效语义不在检索层，在提示词里**——`search()` 默认把已失效的边**一并返回**，
   靠 prompt 里那句「事实在 valid_at~invalid_at 之间有效」交给下游 LLM 自己判断。
   **迁移时只搬 fact 文本、丢掉两个日期字段 = 死事实静默复活。**

### Step 4：claude-mem —— 钩子自动写入

**形态**：SQLite（实测 31 张表）+ Chroma（向量库，**384 维**）。

**它和前面几套最大的区别是「谁触发写入」**：前面都是**模型自觉**（想起来才写），
它是**钩子**——挂在 Claude Code 的 8 个生命周期事件上（会话开始、工具调用前后、会话结束…），
**触发是有保证的**。

**但「触发有保证」≠「一定有条目」。** 实测：只做一次 Read 的会话**产出 0 条**（被判定为噪声
跳过）；让它真干活的会话才落库。

**一条记忆的形态很特别——不是一个字段，是五个**：

```
title      = "Created wordcount.py CLI word counter with error handling"
subtitle   = "Word-count script reads argv[1] file, prints word count..."
facts      = ["Created wordcount.py ...", "Script prints \"error: file not found...\"", ...]  ← 4 条
narrative  = "Wrote a new Python CLI script wordcount.py as part of a lab exercise..."
concepts   = ["what-changed", "pattern", "how-it-works"]
files_modified   = ["/Users/kalin/.../wordcount.py"]
discovery_tokens = 3203              ← 生成这条花了多少 token
generated_by_model = "glm-5.3"
```

mem0 的一句话在这里被**展开成了一份小报告**。

**身份字段是 `project`**（取自 git 仓库名或目录名），**没有用户维度**。

**一个实测发现的挂靠关系**：工具调用表和记忆表**不是一一对应的**。那次会话有 10 次工具调用，
只落了 4 条 observation，另外 6 次的 `observation_id` 是空的。
**别假设「N 次调用 = N 条记忆」。**

**出网**：**每一次工具调用**都要过一次 observer 模型——这是它最激进的地方，且是**默认行为**。

### Step 4b：agentmemory —— 零 LLM 也能记

**形态**：一堆 `.bin` 文件，放在一个叫 `state_store.db` 的**目录**里。每个文件是
**JSON 后面跟一段二进制尾巴**，所以 `json.load` 会报错，得用 `raw_decode` 解析。

**它和 claude-mem 最关键的区别：默认不出网。** 不给任何 API key 也能记住东西，
因为默认用 **BM25**（关键词检索），不需要向量、不需要 LLM。
**这是八套里唯一默认不出网的。**

**两层结构**：
- `observation` —— 原始捕获，带 `importance` / `confidence`
- `memory` —— 提炼后的事实，带 `version` / `supersedes`（取代了谁）/ `strength`

**内容保持原样**——`narrative` 里直接塞原始工具返回，**没有二次转述**。
这正是「零 LLM」带来的好处：**不经过模型，就不会被改写。**

**身份是 `sessionId`**，同样没有用户维度。

### Step 5：Memoria —— 可以「回滚」的记忆

**定位和前面都不同**：前面在解决「怎么记」，它在解决「**记错了怎么办**」。

**形态**：MatrixOne 数据库的表，**宿主机上没有任何文件**。而且**按用户分库**——
每个用户一个库（`mem_u_<hash>`），共享库放注册表和 API key。

**一条记忆**：

```
content            = "我们项目最近把 MongoDB 换成了 PostgreSQL，因为事务要求变高了。"   ← 原文
embedding          = <VECF32(2048)>          ← 维度写进了列类型
user_id            = "memlab-1791105014"
is_active          = 1
superseded_by      = NULL
trust_tier         = "T1"                    ← 治理字段：多可信
initial_confidence = 0.95
observed_at        = 2026-10-04 09:10:15     ← 事实发生时间
created_at         = 2026-10-04 09:10:15     ← 写入时间
```

**三个要点**：
1. **内容是原文，不是转述**（中文原句原样存着）。
2. **维度写进了 schema**（`VECF32(2048)`）——八套里只有它把维度固化了，
   代价是**配错维度就锁死**。
3. **治理字段在数据里**（`trust_tier` / `superseded_by` / `is_active`），不靠外部约定。

**它怎么处理矛盾**：和 mem0 一样**两条并存、不裁决**。但它真正的答案是——**回滚**。

实测流程：写入 R1 → 建快照 → 写入矛盾的 R5 → **两条并存** → **回滚到快照** → R5 消失，只剩 R1。

**快照不是备份，是命名标记**（记下了「打这个标记时库里有几条」）。它不判断哪条对，
而是让**整段历史可以退回去**。

### Step 5b：Panella —— 写入要人批准

**定位最特殊**：不解决「怎么记」，解决「**谁有权限记**」。

**形态**：SQLite + sqlite-vec（在容器里）。

**核心机制是 default-deny（默认拒绝）**：

1. agent 提交一条记忆 → **不直接写**，进待批队列
2. **批准前，这条记忆检索不到**（实测返回空）
3. 由人（operator）用命令行**批准** → 才真正落库

实测闭环：

```
提交候选        → {"queued": true, "approval_id": 4}
批准前检索      → 读不到 ✅（这正是产品在工作）
operator 批准   → durable_id=1db17d08b8...（哈希凭证）
批准后检索      → 读到了 ✅
```

**身份是三段式**：`tenant_id` / `wing` / `room`，比别家的单一 `user_id` 细。

**它的 metadata 是一整个溯源包**（八套里最重的），包括：
- `source_id` / `provenance`（这条是谁提议的、谁批的）
- `content_sha256`（**真正的内容哈希**）
- **`migration_batch_id`** ← 这个字段是**现成的**，它原生就为「批量迁移」留了位置
- `access_queries`（谁在什么时候用什么词查过它）

> ⚠️ 实测踩到的坑：它的 `content_hash` **列名有误导性**——存的**不是内容哈希**，
> 而是审批的 `durable_id`。真正的内容哈希在 `metadata.content_sha256`。
> **看值，别看列名。**

---

## 4. 五条横向规律

这几条是整轮实验最值钱的产出。

### 规律一：身份字段没有两家能对上

| 系统 | 身份是什么 |
| --- | --- |
| server-memory | **无** |
| basic-memory | `permalink`（无用户） |
| mem0 | `user_id` + `run_id` |
| Graphiti | `group_id` |
| claude-mem | `project`（git 仓库） |
| agentmemory | `sessionId` |
| Memoria | `user_id`（按用户分库） |
| Panella | `tenant_id` / `wing` / `room` |

**八套八个答案。** 这是迁移丢信息的重灾区：比如从 mem0（有 `user_id`）搬到 claude-mem
（只有 `project`），**用户维度就静默丢了**。

**结论：「这条记忆属于谁」只能由我们的迁移管道自己赋值，不能从源里继承。**

### 规律二：没有任何一家记录 embedding 模型名

八套全查过，**没有一家的记录里带 embedding 模型名**。要分三种情况说清楚，别一概而论：

| 情况 | 系统 | 说明 |
| --- | --- | --- |
| 记录里有向量，但**无模型名** | mem0（2048 维裸数组）、Graphiti（节点和边各一份）、claude-mem（Chroma 384）、Memoria（`VECF32(2048)` 列）、Panella（`vec0` 虚拟表） | 维度要么没有、要么只体现在类型/配置里，**模型名一律不在数据里** |
| **没有向量**（默认） | agentmemory（BM25）、server-memory（纯子串） | 没向量，自然也没有模型名 |
| 向量在**派生索引**里，不在记录里 | basic-memory（fastembed 的 `bge-small`，索引可重建） | 模型名在工具配置里，不在记忆记录里 |

**共同点是：迁移时你没法从记录本身知道向量是怎么来的。** 这对应本仓库的铁律 5
（**禁止静默重嵌入**）：迁移报告必须显式给出重嵌入计划，不能假设原向量能用。

### 规律三：出网能力差异极大，但和记忆质量不是一回事

| 系统 | 默认出网？ |
| --- | --- |
| server-memory、basic-memory | ❌ 全本地 |
| **agentmemory** | ❌ **零 LLM（BM25）** |
| mem0 | ✅ 抽取 + embedding |
| Graphiti | ✅ 每集多次 |
| claude-mem | ✅ **每次工具调用** |
| Memoria、Panella | ✅ 按配置 |

agentmemory 证明了**不出网也能记住东西**，代价是检索质量下降（关键词匹配，换个说法就找不到）。
**「记忆质量」和「出网」是可以解耦的**——这是个可配置的 trade-off，不是必然。

### 规律四：内容是否被转述，决定信息损失

- **被转述**（mem0、claude-mem）：原始对话丢掉，只剩 LLM 的概括
- **保留原文**（server-memory、basic-memory、agentmemory、Memoria、Panella）：内容原样

最糟的组合是**两次转述叠加**——mem0 的 `data` 已经是 mem0 转述的，搬到 claude-mem
又被 observer 转述一次。**这是「搬记忆」和「搬文件」最本质的区别。**

### 规律五：矛盾处理分三种态度，没有一家自动裁决

1. **不处理**（server-memory、basic-memory）——两条并存
2. **标失效**（Graphiti）——唯一写进语义层的，**但仍不裁决**
3. **给机制**（Memoria 回滚、Panella 人批准）——不判断对错，而是提供「退回去」或「不让你写」

**没有一家是自动裁决的。** 这正好印证本仓库的铁律 7：冲突只做候选聚类，
输出清单给人裁决。

**连专门做记忆的系统都保证不了裁决一致性**——mem0 的 ADD/UPDATE 决策本质是 LLM 判断，
换模型就换行为（我们实测那次就误判了）。这是铁律 7 最有力的现实注脚。

---

## 5. 对本仓库（mem-adaptor）的直接结论

1. **目标侧全都能读，只是成本不同**：文件式最便宜，数据库式要连库，图谱式要会 Cypher。
   没有一套是「读不出来」的——这比网页端源侧好得多（那两家根本拿不到）。
2. **Writer 的验收标准是「写进去能用目标自己的检索找回来」**。所以必须按目标的检索方式调形状：
   server-memory 要字面对得上，basic-memory 要过得了英文 embedding，Graphiti 要过得了
   LLM rerank。
3. **身份字段必须由管道赋值**（规律一），这是迁移报告 schema 里必带的一列。
4. **向量必须重嵌入**（规律二），报告里要给结构化计划。
5. **冲突只聚类不裁决**（规律五），报告里输出冲突清单。
6. **可借鉴的两个现成设计**：Memoria 的**快照/回滚**（对应长期规划的「撤回闭环」）、
   Panella 的**「未批准即不可见」**（可直接当铁律 2「默认 dry-run」闸门的验收断言）。

---

## 附：想看得更深

| 想要什么 | 去哪 |
| --- | --- |
| 每条记忆的**完整真实字段**（五套并排的逐字段矩阵） | [step4-5-report.md](archive/step4-5-report.md) 第 7 节 |
| Step 0–5b 的**逐步骤实验日志**（怎么跑的、踩了什么坑） | [PROGRESS.md](archive/PROGRESS.md) 的「已完成实验记录」 |
| 八套的**五问对照表** | [PROGRESS.md](archive/PROGRESS.md) 顶部的实验矩阵 |
| **源侧**（ChatGPT/Claude/Gemini 导出、本地 harness） | [source-memory-formats.md](source-memory-formats.md)、[codex-memory.md](codex-memory.md) |
| 无导出通道产品怎么抽记忆 | [reader-prompts.md](reader-prompts.md) |
| 外部项目的完整身份与地址 | [ecosystem.md](ecosystem.md) |

**证据分级**：本文档的形态与字段结论来自本机实测（直接打开落盘文件/查库）；版本号以实测为准。
个别产品官方文档与实测不一致的地方（如 agentmemory 的 iii-engine 版本），已按实测记录。
