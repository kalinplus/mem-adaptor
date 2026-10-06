# mem-adaptor

本地优先的记忆迁移工具链。默认先出迁移计划，经用户确认后才写目标，并回读生成可验证的回执。
设计与范围见 [文档入口](docs/README.md) 和 [实现计划](docs/implementation-plan.md)。

## 当前状态

M0–M5 已完成：Rust workspace、三个 Reader、两个 Writer 和 CLI；
5 份 JSON Schema v0 正本、对应 Rust 类型、6 个有效向量和 67 个无效变体。
已跑通 Markdown/ChatGPT/Claude → OKF（Open Knowledge Format，开放知识格式）或
UMP（Universal Memory Protocol，通用记忆协议）JSON 的链路：
`plan → apply → 审批 → 写入 → 回读 → 回执`。`init` 尚未实现。
M3 已接 ZIP 路径检查、schema/字段覆盖校验、常开密钥检测、精确去重、回执链与防复活；
审批与历史验证契约见 [M3 契约补充](docs/m3-contract-proposal.md)。
M4 已实现 Markdown/OKF 家、ChatGPT、Claude 三个 Reader，未知 metadata 保留、逐条字段关联和家目录往返；
M5 已完成 OKF 信封/索引/日志、UMP 官方 schema 校验与回读，原生字段和共享产物参与审批保护。
M0–M5 review 的 7 项问题已修复，当前 87 个测试、构建、格式与 Clippy 通过。
**当前仅用于合成数据**：配置交互和独立真实数据验收仍未完成；PII 检测留到 D2–3。

## 本地开发

需要支持 Rust 2024 edition 的稳定版 Rust，以及 `rustfmt`、`clippy`。

```sh
cargo build
cargo run -- --version
cargo run -- --help
cargo fmt --all -- --check
cargo test -p mem-adaptor-core --test schema_consistency
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

最小链路（路径仅为示例，源、目标、报告目录须彼此分开）：

```sh
cargo run -- plan /tmp/synthetic-source --to okf:/tmp/synthetic-home --report /tmp/plan.json
cargo run -- apply /tmp/plan.json
```

将 `--to` 换成 `ump:/tmp/synthetic-ump`，即可导出 `records.ump.json` 数组。

显式选择密钥策略和读取旧回执：

```sh
cargo run -- plan /tmp/synthetic-source --to okf:/tmp/synthetic-home --report /tmp/next.json \
  --secret-policy block --previous-receipt /tmp/plan.receipt.json
```

`--allow-rule <规则 id>` 可重复传入；白名单只影响处置，不关闭检测或隐藏命中。
规则是 Gitleaks 的六条签名正则加本地密码赋值规则，不等同于完整 Gitleaks 检测器。
源支持目录与 ZIP；ZIP 只验证路径和符号链接，尚无解压资源配额。

`apply` 交互确认后才写入；非交互执行须显式加 `--yes`。
审批凭证和回执默认写在计划报告旁边；已有报告不覆盖。
有 verified 旧回执且目标未改动时才允许按稳定 id 更新；用户删除的目标不重写，
目标侧修改或无历史依据的同名记录输出 unresolved，不自动覆盖。

## Reader 输入与验收产物

- Markdown：一文件一条，保留正文换行；`MEMORY.md` 只登记。OKF 的 `mem_adaptor` 扩展恢复原身份与 metadata。
- ChatGPT：`memory.json`/`saved_memories.json` 支持数组或 `{memory:[...]}`；
  `memories.json` 按内容形状区分来源。Prompt 输出保存为 `*.chatgpt.md`，明确来源，不凭三列文本猜平台。
  日期只保留为自报 metadata，不补造时间；删除条目只计数，禁用条目待裁决。
- Claude：`memories.json` 的 `memory_files` 存在即优先，即使为空；
  缺失才读整块旧版账号/项目记忆。`projects.json` 的文档与指令分别导入。
- 两家的 `conversations.json` 只登记、计数，不抽取聊天；空数组根据同目录的导出文件判定，
  单独一个无来源证据的 `[]` 不猜平台，留在未认领清单。
- 未知 metadata 保存在内部 `source_extra`，报告仅列字段路径、保留位置与规则；
  metadata 同样参与审批摘要和密钥闸门。Claude 文件内容完整保留，不按文件名猜保护类别。

验收入口：`crates/cli/tests/readers.rs`；合成输入及三份可读的稳定计划快照：
`crates/cli/tests/fixtures/m4/`。具体契约与限制见 [M4 方案](docs/m4-reader-proposal.md)。

## Writer 输入与验收产物

- OKF：稳定 id 的 `memories/*.md`、scope 分组的 `index.md`、追加的 `log.md`；
  标题取正文首个非空行，不生成 description 或 verified。原源 metadata 保留；
  向量明确移除，模型/维度/归一化声明与重嵌入计划保留，不调用 embedding。
- UMP：`records.ump.json` 数组，以官方 schema 离线校验；
  保守映射 kind，原类别和全部 metadata 保留在扩展槽。缺失源创建时刻时标明
  `target_migration`，更新不改变首次目标创建时刻，回读不补造源时间。
- `prior_write.target_hash` 检查原生字段；目标文件与共享产物的哈希/字节数进入审批。
  `target_map` 解释目标字段落位，不内联值。未经处理的非空 `consent.redact` 明确拒写。
  空写入回执保留此前共享产物依据，不把用户修改过的文件洗成可覆盖的新基准。
- Writer 返回实际输出字节的证明，引擎在回读及回执阶段对照检查，不把后来的用户修改记作写入。
  自身 `prior_write` 与代表引用 `duplicate_write` 分开保留；已有重嵌入计划不被默认计划替换。
- 不覆盖非受管同名索引/日志，不索引无关 Markdown；UMP 保留数组中无关记录。
  写入使用单文件原子替换，不是跨记录/索引/日志的事务，也不提供跨进程锁。

验收入口：`crates/cli/tests/writers.rs`；
原生产物快照：`crates/cli/tests/fixtures/m5/snapshots/`。
契约、许可来源与限制见 [M5 方案](docs/m5-writer-proposal.md)。

依赖方向：`cli → {reader-*, writer-*} → core`。核心不依赖适配器，适配器之间不互相依赖。
第三方实验代码与真实记忆仍保留在忽略的 `lab/` 数据目录里，不用于产品测试。
schema 的使用与校验边界见 [测试向量说明](schema/vectors/README.md)；
当前已验证结构与类型一致性、摘要重算、密钥报告脱敏和跨次迁移保护。
七项 review 修复与回归范围见 [修复记录](docs/m0-m5-review-fixes.md)。

## 隐私

后续的家目录、计划报告和回执都属于敏感数据，不能放到公开仓库。
抽取和检测默认在本地执行，远程模型必须显式启用。
密钥默认放行，会原样进入目标；阻止写入须选 `--secret-policy block`。
报告不含命中值；源/目标路径或源身份字段含密钥时拒绝生成报告，不改写身份。
检测覆盖 metadata 的键和值，键命中用父路径和 `key_hash` 定位，不在报告内联键名。
所有 JCS 哈希入口拒绝超出 `±(2^53−1)` 的整数，不静默舍入或改成字符串。
