# Step 4–5 配置报告：从「被动工具」到「自动写入 + 治理」

范围：把 Step 4（钩子自动写入：claude-mem、agentmemory）和 Step 5（治理后端：Memoria、Panella）
在本机装起来、跑通、留出你可以亲手体验的入口。

隔离原则（你选的方案）：四套系统各自独立目录，互不共用数据；Claude Code 侧用独立
`CLAUDE_CONFIG_DIR`，不动 `~/.claude` 的插件与钩子，也不动 `~/.claude.json`。

证据分级沿用仓库约定：**实测** = 本次在这台机器上跑出来并核对过的；**待验证** = 配置完成但尚未跑通。

---

## 结论先行

1. **Step 4 两套都实测通过**：claude-mem 与 agentmemory 都是「旁路钩子 + 私有库」形态，钩子
   触发有保证（对比 Step 0/1 的 A 类被动工具），但**真相都不在人类可读文件里**（SQLite / 引擎 state store）。
2. **两套都自带远程模型出口**，且都是默认行为而不是可选项：claude-mem 的 observer 每次工具调用
   都要调一次模型（本次 = 智谱 glm-5.3）；agentmemory 默认反而是**零 LLM**（BM25），这是四套里
   少见的「默认不出网」。迁移报告的 `egress` 字段又添两个样本。
3. **claude-mem 的落库依赖模型守格式**：observer 必须回 `<observation>` XML。单次 Read 的冒烟
   没产出任何 observation（无噪声内容被跳过），换成 4 次工具调用的会话才落库 2 条。这与 Step 3
   Graphiti 的 `json_object` 漂移是同一类问题——**别家的记忆质量取决于别家的模型是否听话**。
4. **治理模型的差异是 Step 5 的核心**：Panella 是 default-deny（写入是提案，要人批准，批准进哈希链），
   Memoria 是 Git 式版本控制（snapshot/branch/rollback）。两者都不是「更聪明的记忆」，而是**把
   写权限与可撤回性做成机制**——正是铁律 7（冲突不自动裁决）与长期「撤回闭环」要借的形态。

---

## 环境与隔离总表（实测）

| 系统 | 形态 | 隔离方式 | 数据/真相位置 | 端口 | 远程出口 |
|---|---|---|---|---|---|
| claude-mem 13.29.0 | 旁路钩子（B 类） | `CLAUDE_CONFIG_DIR=lab/step4-claude-mem/claude-config`<br>`CLAUDE_MEM_DATA_DIR=lab/step4-claude-mem/data` | `data/claude-mem.db`（SQLite）+ `data/chroma` | worker `127.0.0.1:37777` | observer 每次工具调用调 LLM（本次 glm-5.3） |
| agentmemory 0.9.29 | 旁路钩子（B 类） | `CLAUDE_CONFIG_DIR=lab/step4-agentmemory/claude-config`<br>`--data-dir lab/step4-agentmemory/data` | iii state store（`data/`）+ 平台数据目录 | REST/MCP `3111`、streams `3112`、viewer `3113`、engine `49134` | 默认**零 LLM**（BM25）；开 `EMBEDDING_PROVIDER` / provider key 后才出网 |
| Memoria v0.5.2（CLI）+ matrixorigin 镜像 | 常驻服务 + MCP | 独立 compose project，端口 6001/8100 | MatrixOne 容器 volume（无人类可读文件） | MatrixOne `6001`、API `8100` | 抽取/embedding 按 `.env` 配置（本次智谱） |
| Panella 0.2.1 | 常驻服务 + MCP | `PANELLA_HOME=lab/step5-panella/box` | Compose volume `panella-store` | store `127.0.0.1:8000`、facade `127.0.0.1:8001` | 默认无 BYOK 也能跑；embedding 模型**烘进镜像**，零首启出网 |

---

## 五问对照（Step 4–5，沿用仓库模板）

| 五问 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|
| 1 接线/触发 | 旁路钩子（8 个事件，触发有保证）；工具面 = MCP 搜索 3 工具 | 旁路钩子（12 个事件）+ 插件自带 `.mcp.json` 自动接 MCP（54 工具） | 常驻 HTTP 服务 + CLI 做 MCP bridge（25 工具）；`memoria init` 生成 `.mcp.json` | 常驻 store + facade，MCP HTTP；工具面按 profile 给（本 box = `mcp-write`），但写路径仍然只有 propose |
| 2 存储/身份 | `claude-mem.db`（SQLite，observation/summary/tool_uses/prompt 等 30+ 表）+ Chroma；身份 = **project**（git 仓库名或目录名） | iii state store（`data/`）；身份 = session/memory id，`sessionIds` 关联 | MatrixOne（容器 volume，**无人类可读文件**）；身份 = **user_id**（per-user DB + `mem_user_registry`） | Compose volume `panella-store`（`sqlite_vec.db`）；身份 = `tenant_id` / `wing` / `room` |
| 3 变化 | 每次工具调用压成一条 observation；`content_hash` 字节级去重；近似去重 opt-in | 观察 → 记忆（`version`、`supersedes`、`strength`）；默认关掉 consolidation | CRUD + `correct` + **snapshot / branch / checkout / rollback / merge**；矛盾**不裁决**（实测两条并存） | **default-deny**：写 = 提案 → 人批准 → 落库（哈希 `durable_id`）；未批准检索不到 |
| 4 检索 | FTS + Chroma 向量；MCP `search`/`timeline`/`get_observations` | 默认 **BM25**（零 LLM）；开 `EMBEDDING_PROVIDER` 才有向量 | 向量 + 全文混合（实测中文 query「数据库」命中，score 0.633） | 本地 embedding（模型烘进镜像）+ 房间/侧翼过滤 |
| 5 边界 | 每次工具调用都调 observer 模型（本次智谱 glm-5.3）——**默认出网** | 默认**零 LLM**（无 key）；开 provider 才出网 | embedding/LLM 按 `.env`（本次出网智谱） | 默认无 BYOK 也能跑，embedding 模型**烘进镜像**，零首启出网 |

**这张表最该记住的是第 5 问**：四套里只有 agentmemory 默认不出网，其余三套的「记忆质量」
都建立在把内容发给远程模型上。claude-mem 尤其彻底——**每一次工具调用**都是一次出站调用。

---

# 一条记忆到底长什么样：真实落盘逐字段对照

上面那张五问表是「结论」，这一节是「证据」。目标读者是**只想把记忆从 A 搬到 B、不打算读源码的人**：
看完第 7 节那张逐字段矩阵，你应该能说清「每种系统里一条记忆分别长什么样、由哪些字段组成、
两两之间哪几个字段能对上、哪几个必然丢」。

结构：**第 1/2/4/5/6 节是每个系统一份字段档案**（接入新系统时照着写一节），
**第 3 节是推导规则**（写一次，永久复用），**第 7 节是五套并排的逐字段矩阵**（核心 artifact），
**第 8 节是可复现的读取命令**。注意第 7 节比上面的五问表多一列：多了 **mem0**——
它是我们现成的迁移**源**样本，其余四套是**目标**样本。

下面的每个字段都是从**本机真实落盘**里读出来的（不是文档抄的），采样对象就是 Step 2–5 的实验数据。
样本统一用那句 R1 事实，方便横向比：

> `User's project recently switched from MongoDB to PostgreSQL around September 2026 due to increased transaction requirements.`

---

## 1. mem0（Step 2）——一条记忆 = 一句话 + 一个向量

物理形态：**Qdrant 本地文件**（`lab/step2-mem0/qdrant_db/collection/memlab/storage.sqlite`），
`points` 表只有两列 `id` 和 `point`；`point` 是 **Python pickle** 序列化的 `PointStruct`，
所以要读它得先 `pickle.loads`。解出来的一条长这样：

```json
{
  "id": "397fc002-ffc1-4a22-b638-d5713c1b2505",
  "vector": { "": [ 2048 个 float，省略 ] },
  "payload": {
    "user_id":         "kal",
    "run_id":          "memlab-run1",
    "data":            "User's project recently switched from MongoDB to PostgreSQL around September 2026 due to increased transaction requirements.",
    "text_lemmatized": "User's project recently switched from MongoDB to PostgreSQL around September 2026 due to increased transaction requirements.",
    "hash":            "4b2cd4125a5d138a7c9b5dc4b2a01f29",
    "created_at":      "2026-09-29T10:25:03.700191+00:00",
    "updated_at":      "2026-09-29T10:25:03.700191+00:00",
    "attributed_to":   "user"
  }
}
```

逐字段读：

| 字段 | 是什么 | 迁移时的坑 |
|---|---|---|
| `data` | **唯一的记忆内容**，一句 LLM 转述的英文短句 | 原始对话已经被丢掉了，这是不可逆的有损转换 |
| `vector` | 2048 维向量，在 payload **外面**（Qdrant 的字段） | 裸数组，**没有维度标注、没有模型名** |
| `text_lemmatized` | 词形还原后的副本（给倒排用） | 和 `data` 基本重复，可丢 |
| `hash` | 32 位十六进制 = **MD5** | 只是去重指纹，不是内容 |
| `user_id` | 身份字段，本例 `kal` | **这是跨系统最容易丢的东西** |
| `run_id` | 一批导入的批次号 | 溯源用 |
| `created_at` / `updated_at` | 都是**写入时间** | 注意 `data` 里那句 "around September 2026" 是 LLM **幻觉注入**的时间，不是真实事实时间 |
| `attributed_to` | `user` / `assistant` | 谁说的 |

另外它还有一个**变更日志库** `lab/step2-mem0/history.db`（SQLite），一张 `history` 表：

```
id | memory_id | old_memory | new_memory | event | created_at | updated_at | is_deleted | actor_id | role
```

实测 6 行全是 `event=ADD` —— 因为 mem0 把矛盾句 R5 也判成了 ADD（两条并存），所以 `UPDATE`/`DELETE`
这两个值在这批数据里一次都没出现。**要理解「记忆怎么变」，光看主库不够，得连这张日志表一起搬。**

---

## 2. claude-mem（Step 4）——一条记忆 = 一段结构化叙事

物理形态：**SQLite**（`data/claude-mem.db`，实测 31 张表）+ **Chroma**（`data/chroma/`，向量库）。
主表是 `observations`，一条真实行（`id=3`，为节省篇幅只保留非空字段）：

```
id                  = 3
memory_session_id   = openai-compat-bbbc8be7-...-1791114356101   ← 外键指向 sdk_sessions
project             = scratch                                    ← 身份字段（不是 user！）
type                = feature
title               = Created wordcount.py CLI word counter with error handling
subtitle            = Word-count script reads argv[1] file, prints word count, and handles
                      missing-file and missing-argument cases via stderr with exit code 1.
facts               = ["Created wordcount.py in lab/.../scratch that counts words in a file
                       passed as sys.argv[1] using len(text.split())",
                       "Script prints \"error: file not found: {path}\" to stderr and exits 1
                       on FileNotFoundError", ... 共 4 条]
narrative           = Wrote a new Python CLI script wordcount.py as part of a lab exercise in
                      the claude-mem scratch directory. ... (一段 3–4 句的散文)
concepts            = ["what-changed", "pattern", "how-it-works"]
files_read          = []
files_modified      = ["/Users/kalin/github/mem-adaptor/lab/step4-claude-mem/scratch/wordcount.py"]
created_at          = 2026-10-04T11:45:56.101Z
created_at_epoch    = 1791114356101
discovery_tokens    = 3203              ← 生成这一条花了多少 token
content_hash        = d70d3d7aad979b02  ← 16 位内部算法，每会话内唯一（不是内容哈希，推不出）
generated_by_model  = glm-5.3
reinforcement_dates = ["2026-10-04"]    ← 「又见到一次」的日期数组
occurrence_count    = 1
```

和 mem0 对比，**最大的形态差异**：

1. **内容不是一个字段，是五个。** `title`（一行标题）/ `subtitle`（一句话）/ `facts`（JSON 数组，
   一条一个事实）/ `narrative`（散文）/ `concepts`（标签）。mem0 的一句话在这里被「展开」成
   一份小报告。
2. **没有 `user_id`，身份是 `project`**（本例 `scratch`，取自 git 仓库名或目录名）。也就是说
   同一台机器上换个目录就是另一个「记忆空间」，而你没法在同一空间里区分两个用户。
3. **多了执行元数据**：`files_modified`（绝对路径）、`discovery_tokens`（花掉的 token）、
   `generated_by_model`（哪个模型写的）。这些在 mem0 里完全没有。
4. **时间语义不同**：mem0 的 `updated_at` 在这里变成了 `reinforcement_dates` 数组 +
   `occurrence_count`——表达的是「同一件事被反复看到」，而不是「这条被改过」。

关联表（真实存在，读的时候要一起读）：

| 表 | 一条记录是什么 | 和 observations 的关系 |
|---|---|---|
| `sdk_sessions` | 一次 Claude Code 会话：`content_session_id`、`project`、`cwd`、`user_prompt`、`started_at`/`completed_at`、`status` | 1 个会话 → N 条 observation |
| `user_prompts` | 用户敲的每句话（原文），带 `prompt_number` | 按 `session_db_id` 挂到会话 |
| `tool_uses` | 每次工具调用：`tool_name`、`tool_input`（JSON）、`tool_response`（JSON）、`observation_id` | 指向它催生的那条 observation |
| `session_summaries` | 会话级总结：`request`/`investigated`/`learned`/`completed`/`next_steps` | 比 observation 粗一层 |

**实测的挂靠关系**（10 次工具调用 → 4 条 observation）：`tool_uses.observation_id` 是**可空**的——
只有被 observer「采用」的工具调用才有值。实测 10 次里只有 4 次被采用（`Write`×1、`Bash`×1 →
obs 1/2；`Write`×1、`Bash`×1 → obs 3/4），另外 6 次（含全部 `Read`）`observation_id` 为空。
**这说明「工具调用表」和「记忆表」不是一一对应**，迁移时别假设 N 次调用 = N 条记忆。
`tool_uses.content_hash` 和 `observations.content_hash` 也是**两套不同的哈希**，别混用。

向量侧：Chroma 集合 `cm__claude-mem`，**384 维**（注意：不是 2048，也不是 embedding-3）。
实测它把每条 observation **拆成多个文档**存：1 个 `narrative` + 每条 `fact` 各一个，
`doc_type` 都是 `observation`；用户 prompt 另存为 `doc_type=user_prompt`。文档里带的元数据是
`project` / `doc_type` / `sqlite_id` / `created_at_epoch` / `platform_source`——**依然没有模型名和维度标注**。

---

## 3. 怎么用第 7 节的矩阵推出任意 A→B 的迁移计划（方法 + 一个演示）

**先给一个判断：不要为每一对系统写一份走查。** 系统数记作 n，配对数是 n(n−1)，
而且每加一个新系统，就要补 n 份新走查——工作量随系统数平方增长，还会迅速过期。
正确做法是拆成两个都只线性增长的 artifact：

| artifact | 数量 | 是什么 | 什么时候写 |
|---|---|---|---|
| **每系统一份字段档案** | n 份 | 本文第 1/2/4/5/6 节，以及第 7 节矩阵里的那一列 | 接入一个新系统时写一次 |
| **一套推导规则** | 1 份 | 下面这三条 | 写一次，永久复用 |
| 某对系统的具体迁移计划 | 0 份（现推） | 需要迁 A→B 时，拿两份档案按规则推一遍 | 真要做那笔迁移时 |

也就是说：**新加一个系统 = 加一列 + 加一节，不是加 n 节。** 下面这个 mem0→claude-mem
只是「怎么推」的演示，不需要为别的配对复制一份。

### 三条推导规则

拿任意两份字段档案，按顺序过这三条，就能得到一份迁移计划：

1. **身份字段对齐**（最容易静默丢信息的一条）。找出源的身份字段和目标的身份字段，
   问一句「源的每条记忆，落到目标时靠什么字段找回自己的归属」。对不上就是**丢失项**，
   必须在迁移报告里显式列出；不想丢就得在目标侧造命名约定编码进去。
2. **向量重算**。比对两边向量的**维度**和**是否记录模型名**。只要维度不同或模型名缺失，
   原向量就不可复用，属于**重写项**——报告里要给出重嵌入计划（哪些条、用什么模型、质量影响）。
3. **内容是否被二次转述**。如果源的内容字段已经是源方 LLM 转述过的（不是原文），而目标方
   写入时还会再过一次自己的 LLM，那就是**两次有损叠加**，属于**质量风险项**，要写进报告。

### 演示：mem0 → claude-mem

| 规则 | mem0 侧 | claude-mem 侧 | 结论 |
|---|---|---|---|
| 身份 | `user_id` = `kal` + `run_id` | 只有 `project`（无 user 维度） | ❌ 丢失项：用户维度、批次号 |
| 向量 | 2048 维，无模型名 | Chroma 384 维，无模型名 | ❌ 重写项：必须重嵌入 |
| 内容 | `data`（已被 mem0 转述成英文短句） | observer 还会再转述一次 | ⚠️ 风险项：两次有损 |
| 时间 | `created_at`/`updated_at`（都是写入时间） | `created_at`（`updated_at` 语义变成 `reinforcement_dates`） | ⚠️ 语义变化 |
| 谁说的 | `attributed_to` | 无对应 | ❌ 丢失项 |
| 去重指纹 | `hash` = MD5 32 位 | `content_hash` = 16 位内部算法 | ⚠️ 不可比，需重算 |
| 变更历史 | `history.db` 的 `event`/`old_memory`/`new_memory` | 无对应 | ❌ 丢失项 |

同一套规则换成 mem0→Memoria、claude-mem→Panella 也照样走，只是答案不同——**规则是复用的，
答案不是**。这就是不写 n(n−1) 份走查的原因。

---

## 4. agentmemory（Step 4b）——一条记忆 = 一个 JSON 对象

物理形态：**不是 SQLite，是一堆 `.bin` 文件**（`data/state_store.db/` 目录，注意它是个**目录名**）。
每个文件是 **JSON 后面跟一段二进制尾**（长度/校验），所以 `json.load` 会报错，得用
`JSONDecoder().raw_decode()` 解析。文件名本身是 URL 编码的 key，如 `mem%3Amemories.bin`。

**observation**（钩子直接落的，`mem%3Aobs%3A<session>.bin`）：

```json
{
  "obs_mutkbqxp_550e5e17bc6f": {
    "id": "obs_mutkbqxp_550e5e17bc6f",
    "type": "conversation",
    "title": "prompt_submit",
    "narrative": "{\"content\":\"# PostgreSQL Tuning Tips\\n\\n1. **Increase `shared_buffers`**: ...",
    "facts": [],
    "concepts": [],
    "files": ["/Users/kalin/.../scratch/notes.md"],
    "importance": 5,
    "confidence": 0.3,
    "sessionId": "4c406665-0852-4017-8e40-252c99334ccf",
    "timestamp": "2026-10-04T08:32:19.703Z",
    "origin": { "capturedAt": "2026-10-04T08:32:19.703Z", "channel": "tool", "detail": "Write" }
  }
}
```

**memory**（更高一层，`mem%3Amemories.bin`）：

```json
{
  "mem_mutkb9c4_e3f69f0b352a": {
    "id": "mem_mutkb9c4_e3f69f0b352a",
    "title": "agentmemory lab probe: the project database is PostgreSQL, editor is Neovim.",
    "content": "agentmemory lab probe: the project database is PostgreSQL, editor is Neovim.",
    "type": "fact",
    "concepts": ["install-check", "mem-adaptor"],
    "strength": 7,
    "version": 1,
    "isLatest": true,
    "supersedes": [],
    "sourceObservationIds": [],
    "sessionIds": [],
    "files": [],
    "origin": { "capturedAt": "2026-10-04T08:31:56.931Z", "channel": "agent" },
    "createdAt": "2026-10-04T08:31:56.931Z",
    "updatedAt": "2026-10-04T08:31:56.931Z"
  }
}
```

要点：

1. **两层结构**：`observation`（原始捕获，含 `importance`/`confidence`）和 `memory`（提炼后的
   事实，含 `version`/`supersedes`/`strength`）。迁移时要决定搬哪一层——只搬 memory 会丢原始证据。
2. `narrative` 里**直接塞了原始工具返回**（上面那串转义 JSON 就是 Write 的内容），没有二次转述。
   这是它和 claude-mem 最大的不同：**默认零 LLM 意味着内容保持原样，不会被重写。**
3. 身份是 `sessionId`（会话 UUID）和 `sessionIds` 关联，**同样没有 user 维度**。
4. `supersedes` 数组 + `version` 是版本链，`strength`/`importance`/`confidence` 是排序权重。
5. `sessions.bin` 记会话元数据：`cwd`、`firstPrompt`、`observationCount`、`status`。

---

## 5. Memoria（Step 5）——一条记忆 = 数据库里的一行

物理形态：**MatrixOne 容器里的表**，宿主机上**没有任何人类可读文件**。按用户分库：
`memoria_shared`（注册表、API key、插件） + `mem_u_<hash>`（每个用户一个，含 `mem_memories`）。
实测本机有两个用户库。真实行（`mem_memories`）：

```
memory_id          = 01a1062d9e017611b6c268b20881190b
user_id            = memlab-1791105014
author_id          = NULL
subject_id         = NULL
memory_type        = semantic
content            = 我们项目最近把 MongoDB 换成了 PostgreSQL，因为事务要求变高了。
embedding          = <VECF32(2048)>          ← 注意这是带类型的列
session_id         = NULL
source_event_ids   = []
extra_metadata     = {}
is_active          = 1
superseded_by      = NULL
trust_tier         = T1
initial_confidence = 0.95
observed_at        = 2026-10-04 09:10:15.297854
created_at         = 2026-10-04 09:10:15.535163
updated_at         = 2026-10-04 09:10:15.535163
```

要点（这是四套里字段设计最「数据库」的）：

1. **`content` 是原文，不是转述。** 中文原句原样存着（对比 mem0 转述成英文）。因为 embedding
   用的是智谱 `embedding-3`（多语），不需要翻译。
2. **`embedding` 是 `VECF32(2048)` 类型列**——维度写进了 schema。四套里只有它把维度固化了；
   代价是**配错维度就锁死**（我们第一次就差点踩）。
3. **`observed_at` 和 `created_at` 分开**：前者是事实发生时间（**调用方可传**，不传则默认当前时间），
   后者是写入时间。这是双时序的轻量版（对比 Graphiti 的三组时间）。本次没传 `observed_at`，
   所以两者只差 0.24 秒——**要拿到真实事实时间，迁移时必须显式把原对话的时间戳喂进这个字段**。
4. `trust_tier` / `initial_confidence` / `superseded_by` / `is_active` 是治理字段——**谁信这条、
   多信、被谁取代**，都在数据里，不靠外部约定。
5. **快照不是备份，是命名标记**。`mem_snapshots` 表真实行：

```
id = d6b38a81348a4a928815a5cd762b571f
user_id = memlab-1791105014
name = before_rollback
snapshot_name = mem_snap_11_mem_u_003e1_before_rollback
extra = {"memory_count": 1}
status = active
created_at = 2026-10-04 09:10:15.897179
```

注意 `extra.memory_count=1`——快照记下了「打这个标记时库里有几条」。回滚就是退到这个计数状态。
`mem_branches` 表（`name`/`table_name`/`status`）证明分支是**独立表**，不是标记位。
另有 `mem_edit_log`（变更日志）、`mem_api_call_log`（调用审计）、`memory_graph_nodes`/`edges`（图谱）。

---

## 6. Panella（Step 5b）——一条记忆 = 一行 + 一整套审计记录

物理形态：**SQLite + sqlite-vec**（容器内 `/data/sqlite_vec.db`）。表：`memories`（主表）、
`memory_content_fts`（全文）、`memory_embeddings*`（向量，`vec0` 虚拟表）、`memory_graph`。
真实行（`memories`，`id=2`）：

```
id            = 2
content_hash  = 1db17d08b898e2a4af920ae02585a010129ce61aa9146065928ddbb022b24b20  ← 注意：不是内容哈希！
content       = Owner preference: panella lab nonce 2828e139
tags          = permanent,status:active,wing:owner,room:preferences,agent:panella-finalizer,
                mtype:owner_preference,tenant:t_owner_personal,approval_ref:4
memory_type   = observation
version       = 1
confidence    = 1.0
parent_id     = NULL
superseded_by = NULL
deleted_at    = NULL
created_at    = 1791105667.8777382        ← epoch 秒（浮点）
created_at_iso= 2026-10-04T09:21:07.877738Z
```

⚠️ **一个实测出来的命名陷阱**：`content_hash` 这一列**存的不是内容哈希**，而是审批产生的
哈希链凭证 `durable_id`（实测 `content_hash` == 批准时打印的 `durable_id`，两次都对上）。
真正的内容哈希在 `metadata.content_sha256`，且**精确等于 `sha256(content)`**（实测逐字节验证）。
读这张表时别看列名，看值。四个哈希的算法对照：

| 系统 | 字段 | 算法 | 长度 |
|---|---|---|---|
| mem0 | `payload.hash` | MD5（实测 `md5(data)` 完全对上） | 32 |
| claude-mem | `observations.content_hash` | 内部算法 | 16 |
| Memoria | 无内容哈希列 | — | — |
| Panella | `metadata.content_sha256` | SHA-256（实测 `sha256(content)` 完全对上） | 64 |
| Panella | `memories.content_hash` | **审批 durable_id，不是内容哈希** | 64 |

而 `metadata` 是**一整个 JSON 溯源包**（这是四套里最重的）：

```json
{
  "subject_id": "u_owner", "principal_id": "human:owner", "actor_id": "human:owner",
  "tenant_id": "t_owner_personal", "wing": "owner", "room": "preferences",
  "memory_id": "drawer_owner_preferences_0d71f624b10a0db1",
  "content_sha256": "0d71f624b10a0db1ca9335d2c9bfd2e4e53ac48906666ffe17f58423d5a7c73e",
  "source_id": "approval_queue:4", "source_system": "owner-manual",
  "source_file": "approval_queue:4:534bdb8af038e401...", "chunk_index": 0,
  "added_by": "local_cli-approval-bot", "author_agent_id": "mcp-write",
  "agent": "panella-finalizer", "agent_profile": "panella-finalizer",
  "valid_from": "2026-10-04T09:21:07.858509+00:00", "valid_to": null,
  "event_time": "...", "filed_at": "...", "ingested_at": "...", "created_at": "...",   ← 本次这四个值与 valid_from 完全相等，无法区分语义
  "schema_version": "v2", "privacy_scope": "agent-wide", "importance_score": 2.0,
  "access_count": 1, "last_accessed_at": 1791105668.117199,
  "access_queries": [ { "query": "panella lab nonce 2828e139", "timestamp": 1791105668.117199 } ],
  "links": [], "readable_by": [], "migration_batch_id": null,
  "provenance": { "approval_queue_id": 4, "capture": "approved-via-local_cli",
                  "proposed_by_profile": "mcp-write" }
}
```

要点：

1. **`migration_batch_id` 字段是现成的**——Panella 原生就为「批量迁移」留了字段。做 Writer 时
   这个字段直接可用，不用自己造。
2. **`access_queries` 是访问审计**：谁在什么时刻用什么 query 查过这条。这也是我们之前在
   `governed_roundtrip.py` 里踩到的那个「查询词回显」的来源。
3. **溯源链完整**：`source_id` → `approval_queue:4` → `provenance.approval_queue_id` →
   `proposed_by_profile: mcp-write` → `added_by: local_cli-approval-bot`。
   也就是说「这条是谁提议的、谁批的、从哪进来的」全在数据里。
4. `content_hash` 是**审批 durable_id**（不是内容哈希），真正的内容哈希是
   `metadata.content_sha256` = SHA-256（64 位）——见上面的命名陷阱。
5. 身份是三段式：`tenant_id` / `wing` / `room`——比别家的单一 `user_id` 或 `project` 细。
6. **仍然没有 embedding 模型名**：`memory_embeddings*` 是 `vec0` 虚拟表（宿主机读不了，
   需要扩展），`memory_embeddings_info` 里只有 `CREATE_VERSION` 之类的建表版本，
   模型名不在库里。四套全中同一条铁律 5。

---

## 7. 总表：逐字段矩阵（一条记忆的全部字段，五种形态并排）

这是本文档的核心 artifact。**每一行是一个「记忆必须回答的问题」，每一列是一个系统**——
读法：横着读一行，就知道同一个问题五家分别怎么答；竖着读一列，就是这个系统的字段档案。
**接入新系统时加一列，不要加 n 行。**

### 7.1 载体与标识

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 物理载体 | `qdrant_db/collection/memlab/storage.sqlite` | `data/claude-mem.db` + `data/chroma/` | `data/state_store.db/`（**目录**，内含多个 `.bin`） | MatrixOne（MySQL 协议，**宿主机无文件**） | 容器内 `/data/sqlite_vec.db` |
| 存储格式 | `points(id, point)`，`point` 是 **pickle** 的 `PointStruct` | 关系表 + FTS5 + Chroma | 每个 `.bin` = **JSON + 二进制尾**（`json.load` 会报错，需 `raw_decode`） | MySQL 表 | 关系表 + FTS5 + `vec0` 虚拟表 |
| 主表/主文件 | `points` | `observations` | `mem%3Aobs%3A<session>.bin` / `mem%3Amemories.bin` | `mem_memories` | `memories` |
| 记录主键 | `id` = UUID（`397fc002-…`） | `id` = 自增整数 | `id` = `obs_xxxx` / `mem_xxxx` | `memory_id` = `01a1062d9e…` | `id` 自增 + `metadata.memory_id` = `drawer_owner_preferences_<hash16>` |
| 表/文件数量级 | 1 表（+ `history.db` 日志库 2 表） | 31 表 | 每会话一个 obs 文件 + 十余个共享索引文件（memories/sessions/audit/graph/metrics/access/bm25/health…） | 17 表/用户库 + 20 表共享库 | 5 张核心表（另有 FTS5 与 vec0 各 4 张影子表） |

### 7.2 内容

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 内容主字段 | `data`（**一句话**） | `narrative`（散文）+ `facts[]`（JSON 数组，一条一个事实） | `narrative`（原文）/ `content` | `content` | `content` |
| 是否保留原文 | ❌ 已被 mem0 的 LLM 转述 | ❌ 已被 observer 转述（且是**二次**） | ✅ 原样（零 LLM 默认） | ✅ 原样（中文原句） | ✅ 原样 |
| 附加内容字段 | `text_lemmatized`（词形还原副本） | `title` / `subtitle` / `concepts[]` | `title` / `facts[]` / `concepts[]` | — | — |
| 执行元数据 | 无 | `files_read[]` / `files_modified[]`（绝对路径）/ `discovery_tokens`（生成花了多少 token）/ `generated_by_model` | `files[]` / `origin.detail` | — | `metadata.chunk_index` |

### 7.3 身份与归属

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 身份字段 | `user_id` = `kal` | **`project`** = `scratch`（来自目录名/git 仓库名） | `sessionId` / `sessionIds[]` | `user_id` = `memlab-1791105014`（**per-user 分库**） | `metadata.tenant_id` / `wing` / `room`（三段式） |
| 有 user 维度？ | ✅ | ❌ **没有** | ❌ 只有会话 | ✅（靠分库实现） | ✅（`subject_id` / `principal_id`） |
| 批次/分组 | `run_id` = `memlab-run1` | ❌ | ❌ | ❌ | ❌ |
| 谁说的 | `attributed_to` = `user`/`assistant` | `agent_type` / `agent_id`（实测空） | `origin.channel` = `user`/`tool`/`agent` | `author_id` / `subject_id` | `metadata.actor_id` = `human:owner` |
| 会话关联 | ❌ | `sdk_sessions`（`content_session_id` / `cwd` / `user_prompt` / `status`）+ `user_prompts` + `tool_uses` | `mem%3Asessions.bin`（`cwd` / `firstPrompt` / `observationCount`） | `session_id` / `source_event_ids[]` | `metadata.session_id` / `conversation_id` |

### 7.4 时间

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 写入时间 | `created_at` / `updated_at`（ISO） | `created_at` / `created_at_epoch` | `createdAt` / `updatedAt` / `origin.capturedAt` | `created_at` / `updated_at` | `created_at`（epoch 浮点）/ `created_at_iso` |
| 事实发生时间 | ❌（且 `data` 里被**幻觉注入**了 "around September 2026"） | ❌ | ❌ | ✅ `observed_at`（**调用方可传**；不传则默认当前时间，本次没传） | ⚠️ `valid_from` / `event_time` / `filed_at` / `ingested_at` 字段都在，但本次数据里**四者与 `created_at` 完全相等**，无法区分各自语义 |
| 失效时间 | ❌ | ❌ | ❌ | ❌（靠 `superseded_by`） | ✅ `metadata.valid_to`（本次为 `null`） |

### 7.5 变化与版本

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 版本字段 | ❌ | ❌ | `version` + `isLatest` | ❌ | `version` + `parent_id` |
| 取代关系 | ❌ | ⚠️ 只有 `merged_into_project`（跨 project 合并，**不是版本取代**） | `supersedes[]` | `superseded_by` | `superseded_by` |
| 变更日志 | **独立库** `history.db`：`event`(ADD/UPDATE/DELETE) / `old_memory` / `new_memory` | ❌（只有 `reinforcement_dates[]` / `occurrence_count`） | ❌ | `mem_edit_log` 表 | ❌ |
| 删除/失效 | `history.is_deleted` | ❌ | ❌ | `is_active` | `deleted_at` |
| 冲突处理 | 两条并存（R5 判成 ADD） | 不裁决 | 不裁决 | **不裁决，答案是回滚** | 不裁决，答案是人批准 |

### 7.6 检索与向量

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 检索方式 | 纯向量余弦 | FTS5 + Chroma 向量 | 默认 **BM25**；开 provider 才有向量 | 向量 + 全文混合 | FTS5 + `vec0` 向量 |
| 向量维度 | 2048（无标注） | **384**（无标注） | 无（默认零 LLM） | `VECF32(2048)`（**写进 schema**） | 未标注 |
| 向量存放位置 | `PointStruct.vector`（payload **外**） | Chroma `cm__claude-mem` 集合 | `mem%3Aindex%3Abm25.bin`（默认无向量） | `embedding` 列 | `memory_embeddings`（`vec0` 虚拟表） |
| 记录 embedding 模型名 | ❌ | ❌ | ❌ | ❌ | ❌ |
| 一记录拆几个向量 | 1 | 1 + N（`narrative` 1 个 + 每条 `fact` 各 1 个） | — | 1 | 1 |
| 检索到的元数据 | payload 全量 | `project`/`doc_type`/`sqlite_id`/`created_at_epoch`/`platform_source` | — | 全行 | 全行 + `metadata` |

### 7.7 溯源、治理与审计

| 问题 | mem0 | claude-mem | agentmemory | Memoria | Panella |
|---|---|---|---|---|---|
| 去重指纹 | `hash` = **MD5**（32 位，实测 `md5(data)` 对上） | `content_hash`（16 位内部算法，**每会话内唯一**） | ❌ | ❌ | `metadata.content_sha256` = **SHA-256**；⚠️ `content_hash` **列存的是审批 durable_id，不是内容哈希** |
| 来源溯源 | ❌ | ❌ | `origin.capturedAt` / `channel` / `detail` | `source_event_ids[]` | ✅ `source_id` / `source_system` / `source_file` / `provenance{approval_queue_id, capture, proposed_by_profile}` / `added_by` |
| 治理字段 | ❌ | ❌ | `strength` / `importance` / `confidence` | `trust_tier` / `initial_confidence` | ✅ 完整审批链（`approval_ref` tag + `provenance`） |
| 访问审计 | ❌ | `relevance_count` | ❌ | `mem_api_call_log` 表 | ✅ `access_count` / `last_accessed_at` / `access_queries[]` |
| 隐私范围 | ❌ | ❌ | ❌ | ❌ | ✅ `privacy_scope` / `readable_by[]` |
| 快照/分支 | ❌ | ❌ | ❌ | ✅ `mem_snapshots` / `mem_branches` | ❌ |
| 云同步字段 | ❌ | `synced_at` / `origin_device_id` / `sync_rev` / `sync_outbox` | ❌ | ❌ | ❌ |
| **迁移批次字段** | ❌ | ❌ | ❌ | ❌ | ✅ `metadata.migration_batch_id` |

### 7.8 这张表怎么用来做迁移

做 Reader 时**竖着读源列和目标列**，按第 3 节的规则过一遍：

- 凡是「源有、目标无」的行 → 迁移报告里的**丢失项**（如 mem0 的 `user_id`、`attributed_to`）。
- 凡是「维度/算法不同」的行（7.6、7.7 的指纹行）→ **重写项**，必须重算/重嵌入。
- 凡是「源内容已是转述、目标还会再转述」→ **质量风险项**（7.2 的「是否保留原文」行）。
- 凡是目标有、源没有的（如 Panella 的 `migration_batch_id`、Memoria 的 `observed_at`）
  → **填充项**，这是迁移工具要主动补的，不是等对方给。

铁律 6（禁止静默衰减）对应丢失项，铁律 5（禁止静默重嵌入）对应重写项——**这两行是硬约束**。

---

## 8. 怎么自己复现这些读取（都可直接跑）

上面每个字段都能用下面这几条命令复现。**记住：看值，别看列名**（Panella 的 `content_hash`
就是反例）。

```bash
# ---- mem0：Qdrant 的 point 是 pickle，必须用 mem0 的 venv 解 ----
cd /Users/kalin/github/mem-adaptor/lab/step2-mem0
.venv/bin/python - <<'PY'
import sqlite3, pickle, json
con = sqlite3.connect('qdrant_db/collection/memlab/storage.sqlite')
for (blob,) in con.execute("select point from points"):
    d = pickle.loads(blob).__dict__
    print(json.dumps(d['payload'], ensure_ascii=False, indent=2))
    print("vector dim:", len(d['vector']['']))
PY
sqlite3 history.db "select event, new_memory from history;"

# ---- claude-mem：直接开 SQLite ----
DB=/Users/kalin/github/mem-adaptor/lab/step4-claude-mem/data/claude-mem.db
sqlite3 -line "$DB" "select * from observations where id=3;"     # 一条完整 observation
sqlite3 "$DB" "select tool_name, observation_id from tool_uses;" # 工具调用→记忆的挂靠
sqlite3 "$DB" ".schema observations"
# 向量库（Chroma）：集合维度 + 每条的文档类型
python3 -c "
import sqlite3
c=sqlite3.connect('/Users/kalin/github/mem-adaptor/lab/step4-claude-mem/data/chroma/chroma.sqlite3')
print(list(c.execute('select name,dimension from collections')))
print(list(c.execute(\"select string_value,count(*) from embedding_metadata where key='doc_type' group by 1\")))"

# ---- agentmemory：.bin 是 JSON + 二进制尾，用 raw_decode ----
python3 - <<'PY'
import json, pathlib
d = pathlib.Path('/Users/kalin/github/mem-adaptor/lab/step4-agentmemory/data/state_store.db')
for name in ['mem%3Amemories.bin','mem%3Asessions.bin']:
    raw = (d/name).read_bytes().decode('utf-8', errors='ignore')
    obj, _ = json.JSONDecoder().raw_decode(raw)
    print(f"--- {name} ---")
    print(json.dumps(obj, ensure_ascii=False, indent=2)[:1200])
PY

# ---- Memoria：MatrixOne 就是 MySQL 协议，装个 pymysql 直连 ----
# 凭据来自 upstream/.env.example：root:111@127.0.0.1:6001（库名是 per-user 的 mem_u_<hash>）
uv venv /tmp/mo-venv --python 3.12
https_proxy=http://127.0.0.1:7890 uv pip install --python /tmp/mo-venv/bin/python pymysql
/tmp/mo-venv/bin/python - <<'PY'
import pymysql, json
con = pymysql.connect(host='127.0.0.1', port=6001, user='root', password='111', charset='utf8mb4')
cur = con.cursor(pymysql.cursors.DictCursor)
cur.execute("show databases")           # 找 mem_u_<hash> 和 memoria_shared
print([r for r in cur.fetchall()])
cur.execute("describe mem_u_8f20516d683003e1.mem_memories")
print([(r['Field'], r['Type']) for r in cur.fetchall()])
PY

# ---- Panella：库在容器里，借容器的 python3 读 ----
docker exec panella-box-29db0027-panella-1 python3 -c "
import sqlite3, json
con = sqlite3.connect('/data/sqlite_vec.db'); con.row_factory = sqlite3.Row
r = con.execute('select * from memories where id=2').fetchone()
print(json.dumps(json.loads(r['metadata']), ensure_ascii=False, indent=2))
"
```

---

## Step 4 实测记录

### claude-mem（v13.29.0）——观察者把工具调用压成 observation

**怎么装的**（隔离 + 非交互，避免误开云端账号）：

```bash
cd /Users/kalin/github/mem-adaptor/lab/step4-claude-mem
export CLAUDE_CONFIG_DIR="$PWD/claude-config" CLAUDE_MEM_DATA_DIR="$PWD/data" CI=1
npx -y claude-mem@latest install --no-auto-start     # 外网不通先加 https_proxy=http://127.0.0.1:7890
```

安装器把插件装进隔离的 `claude-config/plugins/`，并写好 `data/settings.json`。**注意**：安装器在
无 `--provider` 时会默认「用你已登录的 Anthropic 账号（subscription/keychain）」；本 lab 改成
`openai-compatible` + 智谱 glm-5.3，密钥放 `data/.env`（键名 `OPENAI_COMPAT_API_KEY`），
与 Step 2/3 的模型统一。

**钩子（实测 `plugin/hooks/hooks.json`）**：`Setup` / `SessionStart` / `UserPromptSubmit` /
`PreToolUse(Read)` / `PostToolUse(*)` / `PostToolUseFailure` / `Stop` / `SessionEnd`。

**验证结果**：在 `lab/step4-claude-mem/scratch/` 跑 headless 会话（一次 Read hello.txt、一次
建/跑/改/再跑 `hello.py`），落库 2 条 observation：

| id | type | title | generated_by_model |
|---|---|---|---|
| 1 | change | Created hello.py test script in claude-mem lab scratch directory | glm-5.3 |
| 2 | change | hello.py lab test cycle completed with confirmed outputs | glm-5.3 |

同步进 Chroma（`obs_2_narrative` + 4 条 fact）。`tool_uses`、`user_prompts`、`sdk_sessions` 表同步有记录。

**本次踩到的点**：单次 Read 那种「无实质工作」的工具调用不落 observation（observer 可回
`<skip_summary reason="noise"/>`）；要体验出东西，得让它真干活。日志里 `[DB] STORING ... obsCount=0`
就是被跳过的证据。后来单独跑一次「建个打印 42 的文件跑一下」的会话也是 `obsCount=0`——
钩子、observer、计费都正常（glm-5.3 实耗 2954 tokens），只是内容被判成噪声；换成带错误处理的
`wordcount.py` 会话就落了 2 条（`feature` + `discovery`）。**「0 条」有两种原因，要先分清是哪一种。**

#### 已知坑：`CLAUDE_CONFIG_DIR` 用相对路径会让钩子静默全挂

用户第一次在 `scratch/` 里跑真实会话时，10 次工具调用**一条都没落库**。日志里每次钩子都是：

```
[HOOK] → PostToolUse: Bash(...)
[HOOK] Hook error: The argument 'filename' must be a file URL object, file URL string, or absolute
       path string. Received '../claude-config/plugins/cache/thedotmack/claude-mem/13.29.0/node_modules/noop.js'
```

根因在上游代码：`worker-service.cjs` 用 `createRequire(path.join(pluginRoot, "noop.js"))` 建立
require 锚点，而 `pluginRoot` 是从 `CLAUDE_CONFIG_DIR` 推出来的。传相对路径时它就是相对路径，
Node 的 `createRequire` 直接抛错 —— **钩子在能记录任何东西之前就死了，且不报给用户**。

| `CLAUDE_CONFIG_DIR` | 结果 |
|---|---|
| `../claude-config`（相对） | 钩子每次抛错，observations 不增长（复现过） |
| `$LAB/claude-config`（绝对） | 钩子正常 ENQUEUED，observer 真跑，observations 正常增长 |

**所以本手册里所有 `CLAUDE_CONFIG_DIR` 都写绝对路径。** 这是 claude-mem 13.29.0 的 bug，
不是配置错误，和账号登录无关。

#### 本地化：不需要 CMEM Pro（CloudMem）账号

用户担心的是「安装器要我登录它的云端账号」。实测结论：**claude-mem 可以完全本地跑，不需要
CMEM Pro 账号，也不需要 GitHub 登录。**

| 项目 | 实测值 | 含义 |
|---|---|---|
| `CLAUDE_MEM_PROVIDER` | `openai-compatible` | 不走官方云端 |
| `CLAUDE_MEM_CLOUD_SYNC_TOKEN` | `''` | 不同步到云端 |
| `CLAUDE_MEM_PRO_MEMORY_KEY` | `''` | 不用 CMEM Pro 记忆服务 |
| `CLAUDE_MEM_CLOUD_SYNC_HUB_URL` | `''` | 无云端 hub |
| `data/.env` | 只有 `OPENAI_COMPAT_API_KEY` | 唯一凭据是模型网关的 key |

安装器默认确实会往「登录 Anthropic 账号」那条路走（`--provider` 不传时默认 `claude`），
那是**模型来源**的选择，不是记忆存储的归属——记忆永远落在本地 `data/claude-mem.db`。
唯一需要外网的是 observer 模型（本次走智谱网关）；**若想连模型也不出网，把 provider 换成
本地模型（ollama 等）即可**，记忆存储与云端账号始终无关。

### agentmemory（v0.9.29）——零 LLM 默认值下的自动捕获

**怎么装的**：

```bash
cd /Users/kalin/github/mem-adaptor/lab/step4-agentmemory
export CLAUDE_CONFIG_DIR="$PWD/claude-config"
claude plugin marketplace add rohitg00/agentmemory --scope user
claude plugin install agentmemory@agentmemory --scope user
# 服务端（独立数据目录）
AGENTMEMORY_DATA_DIR="$PWD/data" npx -y @agentmemory/agentmemory@latest --data-dir "$PWD/data"
```

**为什么不用 `agentmemory connect claude-code`**：该命令写的是 `~/.claude.json` 与
`~/.claude/settings.json`（硬编码 `homedir()`），会污染你的日常配置。插件自带 `.mcp.json`，
装进隔离 `CLAUDE_CONFIG_DIR` 就已经把 MCP 接好了，所以跳过了 `connect`。

**验证结果**：
- REST 往返：`POST /agentmemory/remember` 写入 1 条 → `POST /agentmemory/smart-search` 命中。
- 钩子自动写入：headless 会话后 `status` 显示 `Sessions: 1 / Observations: 3`，Memory 1 条，
  Graph 1 node；`Token savings: ~126 tokens (53% reduction)`。
- 默认状态（实测 `status`）：`Provider: noop (no key)`、`Embeddings: bm25-only`，
  `GRAPH_EXTRACTION` / `CONSOLIDATION` / `AUTO_COMPRESS` / `INJECT_CONTEXT` 全是关的。
- 引擎：iii-engine **v0.11.2**（官方文档写 v0.22.1，实际 pin 的是 0.11.2，以实测为准）。

**隔离校验**：你的 `~/.claude/settings.json` 仍是 6 个官方插件 + tokentracker 钩子，
`~/.claude.json` 未被改动；两个 lab 配置各自只启用自己那一个插件。

**隔离的边界（实测，重要）**：agentmemory **只隔离了数据，没有隔离运行时**。

| 项目 | 位置 | 隔离 |
|---|---|---|
| state store / stream 数据 | `lab/step4-agentmemory/data/` | ✅ |
| iii 引擎二进制 | `~/.agentmemory/bin/iii` | ❌ 全局 |
| `iii.pid` / `worker.pid` / `engine-state.json` / `preferences.json` | `~/.agentmemory/` | ❌ 全局 |
| 端口 3111/3112/3113/49134 | 固定 | ❌ 全局独占 |

实测证据：engine PID 18253 的 cmdline 是
`~/.agentmemory/bin/iii --config <npx-cache>/dist/iii-config.yaml`，但该 config 里的
`file_path` 指向 `lab/step4-agentmemory/data/state_store.db`；`~/.agentmemory/iii.pid` 内容
正是 18253。所以**再起第二个实例会撞端口和 pidfile**——它自带的守卫会拦住并提示
`Starting a second instance here would corrupt the running daemon's REST routing`，
这句是正常保护不是故障。`stop` 也读全局 pidfile，停之前先确认停的是哪个实例。

**命令形式（踩过）**：`agentmemory` 不是全局命令，敲 `agentmemory stop` 会 `command not found`；
必须 `npx -y @agentmemory/agentmemory@latest stop`。

---

## Step 5 实测记录

两套的定位完全不同：**Memoria 提供「时间机器」（快照/分支/回滚），Panella 提供「闸门」
（写入是提案，人批准才生效）**。都不是「更聪明的记忆」。

### Memoria v0.5.2 —— Git for memory（快照 / 分支 / 回滚）

**怎么装的**：

```bash
# 1) CLI（从 GitHub Release 装，sha256 校验通过；未加入 PATH）
sh upstream/scripts/install.sh -y --no-telemetry -d lab/step5-memoria/bin

# 2) 栈：用现成镜像，不本地构建 Rust（override 文件替代 build）
cd lab/step5-memoria/upstream
docker compose -f docker-compose.yml -f ../docker-compose.lab.yml up -d
```

`.env` 里的关键选择：embedding 用智谱 `embedding-3`（**实测返回 2048 维**，所以
`MEMORIA_EMBEDDING_DIM=2048`——这一步错了会锁进 schema 无法改），LLM 用 glm-5.3。

**验证结果**（脚本 `lab/step5-memoria/smoke.sh`，可重跑）：

| 步骤 | 结果 |
|---|---|
| mint user key（master key） | 拿到 `raw_key`，`trust_tier=T1`、`initial_confidence=0.95` |
| 写入 R1（MongoDB→PostgreSQL） | 201，`memory_id=01a1062d9e...` |
| 检索中文 query「数据库」 | 命中，`retrieval_score=0.633` |
| 建快照 `before_rollback` | `registered: true`，`memory_count: 1` |
| 写入 R5（又迁回 MongoDB） | **两条并存**，没有语义裁决 |
| 回滚到快照 | `Rolled back to snapshot 'before_rollback'` |
| 回滚后再 list | **R5 那条没了，只剩 R1** |

**核心结论**：Memoria 对矛盾的处理和 mem0 一样是「两条并存」，它真正的答案是
**回滚**——不判断哪条对，而是让整段历史可以退回去。这正是本仓库长期规划里
「撤回闭环（记忆版本链 + 下游传播记录）」要借的形态。

**MCP 接线实测**：`memoria init --tool claude --api-url http://localhost:8100 --token <user key>`
在 `lab/step5-memoria/project/` 生成 `.mcp.json` + `.claude/rules/*.md`（5 条 steering rules）。
注意生成的 `command` 是裸 `memoria`，已改成绝对路径 `lab/step5-memoria/bin/memoria`；
stdio 握手实测返回 **25 个工具**（`memory_store` / `memory_snapshot` / `memory_rollback` /
`memory_branch` / `memory_diff` …）。

### Panella 0.2.1 —— default-deny 的治理箱子

**怎么装的**：

```bash
cd lab/step5-panella/box
PANELLA_HOME="$PWD" uvx panella@0.2.1 up --yes --home "$PWD"
PANELLA_HOME="$PWD" uvx panella@0.2.1 init --verify
```

**`init --verify` 全 PASS（实测输出）**：

```
PASS /v1/health returned 200
PASS /mcp is mounted and refused unauthenticated access with 401
PASS [container] approval transport is local_cli-approvable and stamps an authorized local_cli:owner
PASS [container] MCP profile 'mcp-write' is write-capable (advertises memory.submit_candidate)
PASS approval token file exists with mode 0600 (within 0600) at .panella/approval-token
```

**治理闭环实测**（脚本 `lab/step5-panella/governed_roundtrip.py`，走 MCP HTTP 传输层；nonce 每轮现生成）：

| 步骤 | 结果 |
|---|---|
| agent 可见的工具 | `memory.search`、`memory.submit_candidate`、`memory.list_pending_approvals`、`memory.approve_candidate`、`memory.reject_candidate` |
| 提交候选（nonce） | `{"queued": true, "approval_id": 4}` |
| **批准前检索** | 只命中历史那条，**新 nonce 读不到**（`nonce visible? False`）——这正是产品在工作 |
| operator 批准（CLI） | `approved 4 durable_id=1db17d08b898e2a4af920ae02585a010129ce61aa9146065928ddbb022b24b20`（哈希链凭证） |
| **批准后检索** | 2 条命中，新 nonce 可见（`True`），`content: "Owner preference: panella lab nonce 2828e139"` |

批准后落库的 drawer 带完整溯源：`approval_ref:1`、`source_id: approval_queue:1`、
`added_by: local_cli-approval-bot`、`content_sha256`、`valid_from`/`valid_to`、`memory_id`。

**两个实测到的细节，做迁移工具时会踩**：

1. **drawer 的 metadata 里带检索历史**（谁在什么时刻用什么 query 查过它）。所以判断「某条记忆是否可见」
   不能拿整个响应做子串匹配——查询词自己会被回显，必须只看 hit 的 `content`。这条访问日志本身
   就是审计链的一部分。
2. **批准可能停在中间态**：本次有一次 `approval N is approved but not yet durable (finalize did not
   complete)`，CLI 提示 retry。也就是「已批准」和「已落库」是两步，迁移管道要能识别这种半成品状态，
   否则会把「审批通过」当成「写入成功」。

**核心结论**：Panella 把「写权限」做成了机制——agent 拿不到 approval credential，
默认 `mcp-read`，写入是提案。本仓库铁律 2（默认 dry-run）与铁律 3 对应的验收标准，
可以直接借它这条「**未批准即不可见**」的可验证断言。

**Docker 网络前提（重要，实测）**：这台机器的 Docker 守护进程原本**没有可用代理**，直连
Docker Hub / ghcr.io 拉大层会超时（小镜像与国内镜像源能过，大镜像卡死）。已按你的授权执行：

```bash
orb config set network_proxy http://127.0.0.1:7890   # 还原：orb config set network_proxy auto
```

改完后 daemon 直连 `docker.io` 拉取恢复正常（实测 `python:3.12-slim`、`busybox:1.37` 均直接拉通）。
原有的 `memlab-neo4j`、`easyconnect` 等容器未受影响。

---

## 你（用户）的体验手册

> 你选的是「独立实验目录」，所以下面每条命令都带 `CLAUDE_CONFIG_DIR`，作用是让 Claude Code
> 用 lab 自己的插件配置，**不碰你日常的 `~/.claude`**。外网不通时给命令加前缀
> `https_proxy=http://127.0.0.1:7890`。
>
> ⚠️ **`CLAUDE_CONFIG_DIR` 必须写绝对路径**（`$LAB/...` 或 `$PWD/...`，不要用 `../claude-config`）。
> 写相对路径会让 claude-mem 的钩子**静默全挂**，详见下面的「已知坑」。

### A. claude-mem

```bash
LAB=/Users/kalin/github/mem-adaptor/lab/step4-claude-mem

# 1) 起 worker（默认在 127.0.0.1:37777；viewer 就是它）
cd "$LAB"
CLAUDE_MEM_DATA_DIR="$LAB/data" CLAUDE_MEM_WORKER_PORT=37777 npx -y claude-mem@latest start
open http://127.0.0.1:37777          # 看 observation 实时流

# 2) 在 lab 目录里开一个 Claude Code 会话，让它真的干活（改文件/跑命令）
#    绝对路径！绝对路径！绝对路径！
cd "$LAB/scratch"
CLAUDE_CONFIG_DIR="$LAB/claude-config" CLAUDE_MEM_DATA_DIR="$LAB/data" claude
# 在会话里说：「建一个 fizzbuzz.py 并跑一下，再改成输出 JSON，再跑一次」

# 3) 直接开库看它记了什么（真相在 SQLite）
sqlite3 "$LAB/data/claude-mem.db" \
  "select id,type,title,generated_by_model from observations order by id desc limit 10;"

# 4) 一条都没记？先看钩子有没有报错
grep 'Hook error' "$LAB/data/logs/claude-mem-$(date +%F).log" | tail -5

# 5) 停
cd "$LAB" && CLAUDE_MEM_DATA_DIR="$LAB/data" npx -y claude-mem@latest stop
```

**你要观察的**：observation 的 `title/subtitle/facts/narrative` 是不是把「你做了什么」压成了
一句可复用的结论；粒度是不是「一次工具调用 = 一条」；同一件事做两遍会不会出现两条近似记录。

### B. agentmemory

> **`agentmemory` 不是全局命令**，必须用 `npx -y @agentmemory/agentmemory@latest <子命令>`。
> 直接敲 `agentmemory stop` 会报 `command not found`。
>
> **服务现在已经在跑了**（我起在后台，数据在 `$LAB/data`）。所以**不要**再执行「起服务」那条，
> 否则会撞上它的守卫：
> `agentmemory is already running on port 3111. Starting a second instance here would corrupt the
> running daemon's REST routing.` —— 这句是**正常的保护**，不是故障。先查再决定起不起。

```bash
LAB=/Users/kalin/github/mem-adaptor/lab/step4-agentmemory

# 0) 先查：已经在跑吗？（在跑就直接跳到第 2 步）
curl -s -o /dev/null -w 'REST %{http_code}\n' http://127.0.0.1:3111/agentmemory/health

# 1) 只有在没跑的时候才起（前台跑着，Ctrl-C 停；数据在 $LAB/data）
cd "$LAB"
npx -y @agentmemory/agentmemory@latest --data-dir "$PWD/data"
open http://localhost:3113           # viewer：实时看记忆怎么长出来

# 2) 健康与计数
curl -s http://127.0.0.1:3111/agentmemory/health | python3 -m json.tool | head -20
npx -y @agentmemory/agentmemory@latest status

# 3) 内置演示（造 3 个会话再检索；关键词能中，语义 query 在零 LLM 模式下会空）
npx -y @agentmemory/agentmemory@latest demo --serve   # 自带起服务再跑再停，避免撞守卫

# 4) 导入你真实的 Claude Code 会话记录当记忆（本地解析）
npx -y @agentmemory/agentmemory@latest import-jsonl ~/.claude/projects/<某个slug>/<session>.jsonl

# 5) 在 lab 里开 Claude Code 会话，钩子自动捕获
cd "$LAB/scratch"
CLAUDE_CONFIG_DIR="$LAB/claude-config" AGENTMEMORY_URL=http://127.0.0.1:3111 claude

# 6) 停（也是 npx 形式；注意它停的是全局 pidfile 里记的那个进程）
npx -y @agentmemory/agentmemory@latest stop
```

**隔离程度：只隔离了数据，没隔离运行时。** 实测：

| 项目 | 位置 | 是否隔离 |
|---|---|---|
| 记忆数据（state store / stream） | `$LAB/data/` | ✅ 隔离 |
| iii 引擎二进制 | `~/.agentmemory/bin/iii` | ❌ 全局 |
| pidfile（`iii.pid`/`worker.pid`）、`engine-state.json`、`preferences.json` | `~/.agentmemory/` | ❌ 全局 |
| 端口 3111/3112/3113/49134 | 固定 | ❌ 全局独占 |

也就是说：**数据在 lab 里，但运行时是全局单例**。你自己再起一个 agentmemory 会和这个撞端口
和 pidfile；`stop` 读的也是全局 pidfile，所以停之前先确认停的是哪个实例。

**你要观察的**：默认零 LLM 下 BM25 检索的召回质量（换个说法查不到东西 = 和 Step 0 的字面检索
同一个坑）；打开 `EMBEDDING_PROVIDER=local`（`~/.agentmemory/.env`，注意这是全局路径）后语义
检索的变化；`status` 里那行 token savings 是怎么算的。

### C. Memoria（治理：快照 / 分支 / 回滚）

```bash
LAB=/Users/kalin/github/mem-adaptor/lab/step5-memoria

# 1) 栈状态
cd "$LAB/upstream"
docker compose -f docker-compose.yml -f ../docker-compose.lab.yml ps
curl -s http://localhost:8100/health     # -> ok

# 2) 一键跑完「写入 → 检索 → 快照 → 矛盾 → 回滚」并打印每一步
bash "$LAB/smoke.sh"

# 3) 用 MCP 在 Claude Code 里直接使唤它
cd "$LAB/project" && claude               # .mcp.json 已指向 bin/memoria，首次会问是否信任该项目的 MCP
# 在会话里说：「记住：我们的服务用 PostgreSQL 做事务，用 Redis 做缓存」然后「建个快照 checkpoint-1」
#           再「其实缓存换成了 Memcached」然后「回滚到 checkpoint-1」

# 4) 停栈（保留 volume）
cd "$LAB/upstream" && docker compose -f docker-compose.yml -f ../docker-compose.lab.yml down
```

**你要观察的**：矛盾两条**并存**（它不裁决）；`memory_diff` 在 checkout 到分支后能看到什么；
回滚是整库级还是单条级；`trust_tier` / `initial_confidence` 这些字段谁在写。

### D. Panella（治理：写操作是提案，要人批准）

```bash
LAB=/Users/kalin/github/mem-adaptor/lab/step5-panella

# 1) 自检
cd "$LAB/box" && uvx panella@0.2.1 init --verify

# 2) agent 侧：提交一个候选，并验证「批准前读不到」
cd "$LAB" && python3 governed_roundtrip.py submit   # 打印 tools、approval_id 和 search 结果

# 3) operator 侧：人批准（这一步本来就该是人做的）
cd "$LAB/box"
export PANELLA_BEARER="$(cat .panella/owner-bearer)"
uvx panella@0.2.1 approvals list          # 待批队列
uvx panella@0.2.1 approvals approve <id>  # 批准；若提示 "approved but not yet durable" 再跑一次
uvx panella@0.2.1 approvals reject <id>   # 拒绝

# 4) 再查一次：现在应该读得到
cd "$LAB" && python3 governed_roundtrip.py search

# 5) 停
cd "$LAB/box" && docker compose -p panella-box-29db0027 -f docker-compose.yml down
```

**你要观察的**（Step 5 最值钱的一条）：**未批准即不可见**。提议 → 排队 → 人批准 → 才可召回；
批准产出的是哈希 durable_id。想一想：这套机制能不能直接当成本仓库 dry-run 闸门的验收标准，
以及「审批凭据不交给 agent」这条边界在迁移管道里对应什么。

---

## 体验记录模板（照这个填）

每体验一个系统，往 `docs/PROGRESS.md` 的对应 Step 加一列，或单独记一份：

```markdown
### Step X：<系统名>（<版本>）——<一句话定位>

怎么跑：<目录> / <启动命令> / <喂入方式>
落盘：<真相文件或库，贴一小段真实内容>

五问：
1 接线/触发：
2 存储/身份字段：
3 变化（矛盾/更新/删除）：
4 检索（依赖什么、中文表现、噪声）：
5 边界（出网调用、PII/密钥拦截）：

表格装不下的洞察（3–5 条，每条要有实证）：

自己动手：<命令>
```

**本次要重点回填的三个问题**：
1. claude-mem 的 observation 粒度（工具调用级）与 mem0 的「事实」、graphiti 的「边」怎么对照？
2. agentmemory 默认零 LLM，是不是说明「自动写入」和「出网」可以解耦？解耦后检索质量掉多少？
3. Panella 的「未批准即不可见」能否直接当成本仓库 dry-run 闸门的验收标准？

---

## 清理与回滚

```bash
# Step 4：停服务
cd lab/step4-claude-mem && CLAUDE_MEM_DATA_DIR="$PWD/data" npx -y claude-mem@latest stop
# agentmemory 不是全局命令，必须走 npx；停之前先确认全局 pidfile 指的是哪个实例
npx -y @agentmemory/agentmemory@latest stop

# Step 5：停栈（保留 volume）
cd lab/step5-memoria/upstream && docker compose -f docker-compose.yml -f ../docker-compose.lab.yml down
cd lab/step5-panella/box && docker compose -p panella-box-29db0027 -f docker-compose.yml down

# 卸插件（只影响 lab 配置）
CLAUDE_CONFIG_DIR=lab/step4-claude-mem/claude-config claude plugin uninstall claude-mem@thedotmack
CLAUDE_CONFIG_DIR=lab/step4-agentmemory/claude-config claude plugin uninstall agentmemory@agentmemory

# 还原 Docker 网络代理
orb config set network_proxy auto
```

`lab/*/upstream` 是只读参照源码（`git clone`），本仓库没有 git 元数据，删掉不影响实验结论。
