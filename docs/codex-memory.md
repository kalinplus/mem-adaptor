# Codex 本地记忆（memories）格式

范围：**Codex 本地客户端**（CLI、IDE 扩展、桌面端）。ChatGPT 网页端的记忆是另一套存储与控制，见 [source-memory-formats.md](source-memory-formats.md)。

核实状态分三级，正文逐条标注：

- **官方**：developers.openai.com 上的 Codex 文档（本机直连 403，内容经浏览器会话取得，未二次直连核对）。
- **源码**：openai/codex 仓库里的 memory 相关 README 与 crate 文档，属实现细节，**不是对外契约**，随版本可改。
- **实测**：本机 `~/.codex/` 落盘与 codex 二进制（cli 0.142.5）直接检查的结果。

## 一句话

Codex 的本地记忆是从历史会话生成的 **Markdown 文本资料**，启用后作为后续任务的上下文注入。官方确认它是可读 Markdown，但**没有承诺固定文件布局，也没有承诺 `id`、`owner`、`embedding`、时间戳等字段的稳定契约**。所以对 mem-adaptor 来说它是「版本相关的源产物」，不是可以依赖 schema 的接口。

## 1. 格式与字段

位置：`$CODEX_HOME/memories/`，默认 `~/.codex/memories/`。**官方**将它描述为生成状态，包含四类内容：

| 内容 | 用途 |
| --- | --- |
| 摘要 | 快速恢复以前工作的背景 |
| 持久记忆条目 | 保存可复用的偏好、知识与经验 |
| 最近的输入 | 为后续记忆整合提供材料 |
| 历史会话的支持证据 | 保留记忆的来源依据 |

这四项是**内容类别，不是 JSON 字段名**。

**实测**：本机 `~/.codex/memories/` 目录存在但为空，拿不到真实产物来核对字段形状。空的原因是 §2 的默认关闭。

**源码**（供实现参考，不作契约）：记忆工作区里的文件包括 `MEMORY.md`、`memory_summary.md`、`raw_memories.md`、`rollout_summaries/*.md`、`skills/`、`extensions/`；整个目录被当作一个 git baseline（`~/.codex/memories/.git`），Phase 2 用 git diff 判断要不要跑整合。中间态落在 `~/.codex/memories_1.sqlite` 的 `stage1_outputs` 表：

```
thread_id, source_updated_at, raw_memory, rollout_summary, rollout_slug,
generated_at, usage_count, last_usage, selected_for_phase2,
selected_for_phase2_source_updated_at
```

这是目前能拿到的唯一「逐条字段」证据，**只来自源码，不是官方承诺**。

## 2. 如何新增、变化

启用后，Codex 在**后台**从符合条件的旧会话抽取记忆，再做全局整合。不是每结束一次聊天就立即更新。

**官方**当前配置的默认值：

| 参数 | 默认 |
| --- | --- |
| 会话进入记忆候选前的最小空闲 | 6 小时 |
| 纳入考虑的会话时间窗 | 最近 30 天 |
| 每次启动最多处理的候选数 | 16 个 |
| 全局整合最多保留的近期原始记忆 | 256 条 |
| 记忆未使用多久后失去整合资格 | 30 天（**不等于**官方承诺届时删除文件） |
| 剩余额度低于多少可以跳过生成 | 25% |

抽取与整合可以分别配模型：`memories.extract_model`、`memories.consolidation_model`。文档没有定义逐条新增/修改/删除的公开 CRUD 接口，也没有承诺冲突、版本与覆盖规则。

开关（**本地记忆默认关闭**）：

```toml
[features]
memories = true

[memories]
generate_memories = true
use_memories = true
```

**实测**：本机 `~/.codex/config.toml` 的 `[features]` 段只有 `fast_mode` 和 `multi_agent`，没有 `memories`——所以记忆管线从未运行，这与 `memories/` 为空、`stage1_outputs` 0 行完全一致，不是版本缺失。在 codex 二进制（0.142.5）里以下键全部存在：`generate_memories`、`use_memories`、`extract_model`、`consolidation_model`、`max_unused_days`、`shell_environment_policy.ignore_default_excludes`。

**结论**：「请记住这个」不能视为立即写入持久存储的保证。需要**始终**执行的规则应写进 `AGENTS.md`。记忆文件可以检查，但官方不推荐以手改文件作为主要管理方式。

## 3. 如何取回、作用到 agent

**官方**确认：启用使用记忆后，Codex 会把已有记忆注入未来会话供模型参考；**没有公开精确的检索算法**，因此不能断言它用向量数据库、固定召回条数或某个相似度阈值。

它影响的是模型的判断（沿用偏好、恢复项目背景、复用此前经验）。这属于**上下文输入**，不是修改模型权重，也不赋予工具权限。

生成与使用可以分别控制，`/memories` 控制当前会话是否使用旧记忆、是否作为未来记忆的输入，**不改变全局设置**。

`AGENTS.md` 则有明确的发现与加载规则：全局指引 + 项目指引按目录层级进入提示词，适合必须持续适用的约束。

**实测**（AGENTS.md 侧，可直读）：

- 全局 `~/.codex/AGENTS.md`，也支持 `AGENTS.override.md` 抢占，以及 `config.toml` 里配 fallback 文件名。
- 项目侧从 project root（`.git` 等标记）走到 cwd，**每层目录取一个**文件。
- rollout transcript（`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`）的 `turn_context.payload.user_instructions` 字段记录了那一轮**实际注入的 AGENTS.md 全文**——现成的指令记忆 provenance。
- 本机 `~/.codex/AGENTS.md` 与 `~/.claude/CLAUDE.md`、`~/.gemini/GEMINI.md`、`~/.pi/agent/AGENTS.md` md5 完全一致，说明这层目前由人维护。

## 4. 哪些内容可能进入远程调用

使用远程模型时，把以下内容视为**可能出站**：

| 阶段 | 可能进入模型的内容 |
| --- | --- |
| 日常执行任务 | 用户消息、加载的指令、被读取并返回到上下文的文件内容、工具输出、取回的记忆 |
| 从会话抽取记忆 | 用于抽取的历史会话材料；官方没有逐字段列出发送范围 |
| 全局整合记忆 | 用于整合的原始记忆等材料；精确请求结构未公开 |

本地存储不等于本地推理。文件只存在磁盘上不意味着整个目录会上传；但文件内容一旦被工具读取并进入模型上下文，就应按可能出站处理。

**沙箱的「网络关闭」限制的是工具执行的网络访问，不能当作关闭 Codex 自身远程推理连接的开关。**

## 5. 密钥会不会被拦截

要区分三个位置：

| 位置 | 现状 |
| --- | --- |
| 生成记忆时 | 官方明确说会对**生成的记忆字段**脱敏，但仍要求检查记忆产物 |
| 启动 shell 子进程时 | 有环境变量过滤配置。当前文档中 `shell_environment_policy.ignore_default_excludes` 默认是 `true`；设为 `false` 才应用对名字含 `KEY`、`SECRET`、`TOKEN` 的自动排除。这只过滤子进程环境变量 |
| 发送模型输入前 | 官方资料里没有「所有用户文本、文件内容和工具输出都经过统一密钥扫描并阻断」的保证。不能依赖它防止 `.env`、日志或命令输出中的密钥出站 |

典型误判：工具输出里出现密钥，随后生成的记忆把它脱敏了，**不代表此前模型输入中的密钥也被拦截了**。

## 6. 对 mem-adaptor 的直接结论

1. **Codex 记忆是版本相关的源产物**：Reader 只读、不假设 schema；字段缺失就记缺失，不问「为什么没有 `id`」。
2. **导出前必须跑本项目自己的 PII/密钥出站检查**，不能把 Codex 的记忆脱敏当作安全边界。「源系统不设卡」在这里再次成立，只是卡的位置比别家多一层（生成后脱敏）。
3. **抽取阶段本身就出网**：迁移报告的 `egress` 字段应记「源侧已出网，且出网内容不可逐字段追溯」。
4. **AGENTS.md 是可靠的那一半**：有明确发现与加载规则、可直读、有 `user_instructions` provenance，D1 可以先只吃这层。
5. **实际用一用才知道怎么样**：本机当前是「功能在、开关关、无数据」。要先打开 `[features] memories = true`，再攒够会话（候选要求 ≥6 小时空闲）把它触发出来，才能验证真产物与上面源码推导的文件布局是否一致。

## 待核实

| # | 问题 | 怎么核实 |
| --- | --- | --- |
| 1 | `memories/` 的真实文件布局与内容 | 打开 `[features] memories`，跑几次会话 + 等 6 小时后看产物 |
| 2 | 官方六个默认值的准确取值 | developers.openai.com 直连 403，需换通道二次核对 |
| 3 | 记忆注入的检索形态（关键词 / 向量 / 全量） | 产出真实 `MEMORY.md` 后观察注入内容与源文件的对应关系 |
| 4 | 冲突、版本与覆盖行为 | 人为制造矛盾事实喂两次，看整合结果 |
| 5 | 生成记忆中脱敏的实际覆盖范围 | 喂已知格式的假密钥，检查记忆产物与模型输入两侧 |
