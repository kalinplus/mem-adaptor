# M5：完整 OKF 与 UMP Writer（已确认、已实现）

用户已确认三项推荐：保守 kind 映射、正文首行标题、缺失创建时刻时使用明确标注的目标迁移创建时刻。
用户也已确认 §5 推荐的完整目标哈希、共享产物审批绑定与目标字段映射契约，已完成合成验收。

M4 已完成合成验收，62 个测试通过。M5 不读取真实记忆，不调用模型，不改变审批与回执链。
字段依据已核对的 OKF（Open Knowledge Format，开放知识格式）约定，
以及 UMP（Universal Memory Protocol，通用记忆协议）1.0 的官方 JSON Schema。

## 1. UMP 类别映射

推荐保守映射，不根据正文或日期猜测：

| 源类别 | UMP kind |
|---|---|
| profile | identity |
| preference | semantic |
| instruction | procedural |
| project / tool / project_doc | semantic |
| 已明确属于 UMP 五种类别 | 原类别 |
| 未知或缺失 | semantic，明确标记默认映射 |

原 `source_kind` 与 `dna_class` 保留在目标 metadata，回读可恢复。
项目背景可能是长期事实，所以默认不映射成 working（临时工作上下文）。

可选路径：

1. **保守映射（推荐）**：只有明确的身份/程序性要求采用对应 kind，其余常规事实为 semantic。
2. **project → working**：更贴近短期项目上下文，但可能把长期背景误标成临时信息。
3. **全部 semantic**：最简单，但目标侧无法按身份/程序性记忆分类。

## 2. OKF 标题

推荐取正文首个非空行，去掉标题标记后按 Unicode 字符截断至 80 字；
正文不变，空正文使用 `Memory <canonical_id>`。不生成 description，不调用模型。
原始 title/name/description metadata 继续保留，不被派生展示标题覆盖。

可选路径：

1. **正文首行（推荐，与实现计划一致）**：确定性、无远程调用，适合统一显示。
2. **优先原始标题**：标题更贴近来源，但需为三个源分别规定 title/name 的优先级。
3. **仅稳定 id**：避免把正文带进索引标题，但可读性最差。

## 3. UMP 必填创建时刻

官方 schema 要求 `time.created` 是 RFC 3339。M4 的许多记录没有创建时刻，
Prompt 日期也不能补成午夜。

推荐有可信记录创建时刻就使用它；否则填**目标迁移创建时刻**并明确标注含义。
这是目标记录时间，不是源事实时间；源 `created_at` 的缺失状态仍能回读恢复，
`observed_at` 等事实时间不补造。目标更新时保留原目标创建时刻。

另一条路径是：无源创建时刻的条目保持 unresolved，不输出 UMP。
不能用固定 epoch 或把日期补成午夜伪装成源时间。

## 4. 实现切片与验收

1. OKF：完整信封字段落位、scope 索引、追加迁移日志；未知字段保留，不伪造 verified。
   embedding 向量不进家，模型与维度保留，变化进入计划与回执。
2. UMP：只写 `*.ump.json` 数组，保留可逆 id 与不可信 owner 的边界；
   metadata 按 UMP 扩展槽承载，回读复核。`consent.redact` 不得当装饰字段，
   无法安全执行的要求不得静默忽略。
3. 将官方 schema 和 Apache-2.0 署名随 Writer 分发，测试/运行不依赖 `lab/upstream`。
4. CLI 注册 UMP；两种 Writer 产物有合成快照、官方 schema 校验、往返和历史更新测试。
   所有写入仍先计划、再显式审批；完整 workspace 验证通过才标记完成。

## 5. 完整目标格式的保护契约（已确认）

现有 `record_hash` 只覆盖回读后的 canonical。M5 新增的原生字段（例如 OKF 展示标题、
UMP 的目标创建时刻）以及共享 `index.md`/`log.md` 不一定进入 canonical。
仅靠原哈希，用户在目标侧改这些字段后，可能无法发现变更并在源更新时被覆盖。
这不是格式转换细节，而是 M3 的审批绑定与防覆盖约束必须延伸到的新边界。

推荐一次补齐：

1. `PriorWrite.target_hash`：记录上次实际写出的目标记录载荷哈希。
   OKF 对整份 Markdown 字节取哈希；UMP 对该条原生 JSON 做 JCS 后取哈希。
   下次计划和实际覆盖前都核对，原生字段变化同样 `target_modified`，不自动覆盖。
2. `TargetSpec.artifacts`：计划摘要绑定本次目标中将涉及的记录文件与共享索引/日志的路径、
   哈希和字节数。审批后这些文件变化或出现同名文件，先拒写，而不是重建覆盖。
   不收集正文，不建立额外状态账本，也不触碰未属于 Writer 的其他文件。
   共享文件只有确认是 Writer 管理的产物才允许重建；其他同名文件保持不写，报告待处理。
3. 计划和回执可带逐条 `target_map`：用 canonical/目标 JSON Pointer 和规则名解释类别、
   标题、迁移创建时刻及扩展槽落位，不内联值。预测中的同一映射参与摘要。
4. 引擎对有损转换以 Writer 的预计回读记录做历史相等判断，
   防止“向量明确移除后，下次又误认为源变化”的重复更新。

替代路径是保守降级：M5 的非空目标不更新，共享索引/日志不覆盖。
这不会丢用户修改，但不能满足完整的持续汇总与增量更新验收，不推荐。

v0 尚未发布；已直接修正涉及的 schema 正本、Rust 契约与合成向量，
不兼容旧回执，不改已确认的 dry-run、审批、删除防复活和无自动冲突裁决原则。

## 6. 实施与验收

- CLI `--to okf:<目录>` / `--to ump:<目录>` 均已接通；`apply` 从报告恢复 Writer 类型。
- OKF 补齐信封、scope 索引与追加日志，向量明确移除，模型/维度/归一化声明保留。
  派生原生字段与 metadata 不一致时，Markdown Reader 拒绝静默导入。
- UMP 数组逐条通过官方 schema；源 metadata 和向量在扩展槽恢复。
  目标更新保留首次 `time.created` 与 `created_origin`，不把迁移时间变成事实时间。
- WriteToken 同时绑定允许写入的批次和目标快照；Writer 对实际读取的字节核对审批，
  并在单文件替换前再核对。没有未经处理的 `consent.redact` 执行器，非空要求明确拒写。
- 计划 artifacts 表示审批时现状；回执共享 artifacts 是写入历史依据。
  无 verified 写入时保留此前共享快照，不把受阻后观察到的用户修改洗成新依据。
- `crates/cli/tests/writers.rs` 有 16 个验收测试；
  `crates/cli/tests/fixtures/m5/snapshots/` 四份最终产物已通读核对，只有运行时刻被归一化。
  M4 计划快照补齐新字段、能力声明与重算后的摘要。
- 官方 schema/license 原样复制自 commit `5defe7839dd09da255744c24b0166e600e8e56cd`；
  来源见 `crates/writer-ump/schema/README.md`。不声称实现 UMP L1–L3。
- 78 个 workspace 测试、构建、格式、Clippy、依赖边界及合成 fixtures 可见性检查通过。
  60 个无效 schema 变体覆盖原生哈希、快照 hash/bytes 成对与无内联值的目标映射。

**限制**：写入是单文件原子替换，不是跨文件/跨目标事务，也没有跨进程锁；
途中失败可能留下已成功写出的部分产物，不会自动回滚用户目录。
真实数据验收与独立 conformance 仍待 M7；PII 检测不在 M5。

**Review 后修复**：七项问题已修复，当前 87 个测试通过，见
[修复记录](m0-m5-review-fixes.md)。Writer 返回实际输出字节的证明，并在回读与回执阶段核对；
不再将成功写入之后的用户编辑采样为新基准。自身历史与重复代表引用分离，
受管记录写前预检，已有重嵌入计划保持不变；无效 schema vectors 增至 67 个。
