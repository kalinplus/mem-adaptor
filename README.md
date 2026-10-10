# mem-adaptor

你的 AI 记忆现在散落在各处：Claude Code 各项目的 `memory/` 目录、ChatGPT 的数据导出、
Obsidian 笔记、Claude.ai 的项目记忆……换工具的时候搬不走，也合不到一起。

mem-adaptor 是搬记忆的施工队：把记忆从这些地方读出来，归一、去重、把冲突列清楚，
再按你选的目标格式写进去。它自己**不是**又一个记忆系统——没有云端、没有新发明的档案格式，
搬完只留下一沓可验证的回执。

## 先说脾气（设计底线）

- **默认不动你的数据。** `plan` 只出报告，一个字节都不写；真要写盘得 `apply`，
  交互确认，非交互必须显式加 `--yes`。
- **全程本地。** 解析、去重、检测都在你机器上跑，不调用任何远程模型。
- **冲突不替你做主。** 疑似重复只聚类、列成冲突清单，由你逐条裁决；
  绝不自动合并。某条记忆这轮没出现也不等于删除——想删得你明说，删过的不会复活。
- **每一步留回执。** 写了什么、从哪来、写完回读是否一致，都落在带 schema 的报告里；
  下一次运行拿旧回执当依据：迁过的不再重写，你在家里的修改会被保留。
- **密钥检测常开。** 命中必报，默认放行（`init` 时可选改为拦截，选择记进报告）；
  报告里永远不出现命中值本身。

## 装与用

需要 Rust stable（2024 edition）。

```sh
cargo build
```

最常见的用法是「家模式」：选一个 OKF 目录当家，其他来源都是卫星，定期往家汇总：

```sh
# 建家（生成 index.md / log.md / .mem-adaptor/，顺便问一次密钥策略）
./target/debug/mem-adaptor init ~/my-memory

# 第一颗卫星：某个 Claude Code 项目的记忆
./target/debug/mem-adaptor plan ~/.claude/projects/-Users-me-myproject/memory \
  --to okf:~/my-memory --satellite new --label myproject

# 看过计划报告后写入（plan 结束时会打印报告路径）
./target/debug/mem-adaptor apply <计划报告路径> --yes
```

以后同步不用再带 `--satellite`，直接 `plan` 会认出已登记的卫星：没变的跳过，
源头改过的原位更新，你和家两头都改过的变成冲突等你裁决。
直迁（源 → UMP JSON，不经过家）把 `--to` 换成 `ump:<目录>` 即可。

退出码是给脚本用的：`0` 干净完成；`3` 有被拒/未决/回读不一致；
`4` 依据过期（比如计划生成后目标又被改过）；`1` 输入或 IO 出错；`2` 用法错误。

## 现在支持什么（v0.1.0）

| | 格式 |
|---|---|
| 读 | Markdown 目录（Claude Code `memory/`、Obsidian、OKF 家读回）；ChatGPT 数据导出（`memory.json` 等）；Claude.ai 导出（`memories.json`、`projects.json`） |
| 写 | OKF（Markdown + frontmatter 的家目录）；UMP JSON 数组 |

第一版拿真实数据验收过：11 个 Claude Code 项目的 41 条记忆汇进一个家，
回执全部 `verified`，第二轮全部幂等跳过，原有文件分毫未动。

还没做的：PII 检测、Mem0 / basic-memory 写入端、语义级去重（现在是精确去重 + 冲突聚类）、
家往卫星分发。计划见 [实现计划](docs/implementation-plan.md)。

## 文档

- 设计为什么长这样：[docs/README.md](docs/README.md)（按「你想做什么」组织）
- 各家记忆格式的字段级细节：[source-memory-formats.md](docs/source-memory-formats.md)、
  [memory-products.md](docs/memory-products.md)

## 开发

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

当前 273 个测试全绿。schema 以 `schema/` 下手写的 JSON Schema 为正本，
配 7 个有效向量和 92 个无效变体。

## 隐私

家目录、计划报告、回执都是敏感数据，别放进公开仓库。工具不上传任何东西；
`lab/` 里的第三方实验代码与真实记忆数据在 git 忽略清单里。
