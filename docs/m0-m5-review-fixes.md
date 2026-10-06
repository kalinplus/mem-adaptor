# M0–M5 review：七项修复（已实施）

只读 review 对七项问题均做了仓库外合成复现，用户确认修复方向后授权实施。
本次只修复 M0–M5，不推进 M6，不读取真实记忆。

## 修复与验证

| 问题 | 修复 | 回归位置 |
|---|---|---|
| metadata 键名绕过 block | 扫描键和值，原始源未承载键同样扫描；键命中以父路径和 `key_hash` 定位，路径脱敏 | `crates/cli/tests/review_regressions.rs` |
| 代表更新后别名旧正文被丢弃 | 先生成全部预计回读记录，再统一去重；分叉独立承载，历史删除/修改仍优先阻止写入 | 同上 |
| 新代表替换自身历史，造成防复活失效 | 自身 `prior_write` 与 `duplicate_write` 分离，优先已有 verified 且无需更新的代表，保留真实目标身份 | 同上，含连续代表切换与自身更新 |
| 大整数在 JCS 哈希中丢精度 | 统一 `core::jcs` 入口及 schema 递归数值约束，拒绝超出安全整数范围，不舍入或转字符串 | `crates/core/src/jcs.rs` 单元测试、review 回归与 schema vectors |
| 回执洗白 Writer 完成后的修改 | `WriteResult` 返回实际输出字节生成的 artifacts；`Written` 带原生 `target_hash`；回读前及回执前对照，不采纳后来观察值 | `crates/cli/tests/writers.rs` 的三个注入阶段 |
| 无关 base32 文件名误被索引，失败发生在写入后 | 名称仅筛候选，显式信封标识确认受管；无关 Markdown 跳过，损坏受管记录在计划/写入前拒绝 | review 回归 |
| 已有重嵌入计划被默认值覆盖 | 已有计划完整保留，只在缺失时生成默认计划，向量省略仍明确报告 | Writer 回归 |

## 契约说明

- `Finding.key_hash` 存在表示对象键命中，`field_path` 是父对象路径，`byte_span` 是键内的字节范围；
  不存在则沿用字段值定位。原键不作为新字段值写入报告，映射路径仍脱敏。
- `prior_write` 只表示该 canonical 记录自身真实写出的目标历史。
  `duplicate_write = {canonical_id, prior_write}` 是另一条代表记录的写入依据；
  两者可同时保留，不互相替换。计划预测与 WriteToken 批次哈希绑定代表引用。
  旧版把代表历史放进自身 `prior_write` 的回执不自动迁移或信任。
- `WriteResult.artifacts` 由 Writer 使用准备写出的最终字节计算；
  `Written.target_hash` 是该条原生载荷的哈希。只重读目标再计算证明会重复旧漏洞。
  引擎对观察结果与证明核对，变化时报错，不产出可将用户修改洗白的新成功回执。
- 安全整数范围为 `[-9007199254740991, 9007199254740991]`；
  整数值的科学计数法同样受限，普通有限非整数沿用 JCS 双精度语义。
  源 canonical、记录/去重/批次/计划哈希及现有 UMP 原生记录均检查。
  schema 的递归 `jcs_value` 使任意 metadata 的范围也可由独立语言验证。
- 不索引无关 Markdown；明确声明受管却损坏的文件不能靠吞掉解析异常跳过。
  单文件替换仍不是跨文件事务，不引入跨进程锁或额外状态账本。

## 最终检查

- 87 个测试通过：原 78 项保留，新 JCS 单元 1 项、review 回归 7 项、已有重嵌入计划 1 项。
  原 Writer 竞态测试扩为引擎检查后/Writer 写完后/回读后的三个注入阶段，两个目标都覆盖。
- `cargo test --workspace --offline`、`cargo build --workspace --offline`、
  `cargo clippy --workspace --all-targets --offline -- -D warnings`、格式检查通过。
- 6 个有效 vectors、67 个无效变体通过；三份 M4 计划快照和四份 M5 原生产物快照
  未重录，现有内容继续匹配实际结果。
- 未提交、未推送，M6 仍暂停；真实导出和独立 conformance 尚待 M7。
