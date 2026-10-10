# mem-adaptor

你的 AI 记忆现在散落在各处：Claude Code 各项目的 `memory/` 目录、ChatGPT 的数据导出、
Obsidian 笔记、Claude.ai 的项目记忆……换工具的时候搬不走，也合不到一起。

mem-adaptor 是搬记忆的施工队：把记忆从这些地方读出来，归一、去重、把冲突列清楚，  
再按你选的目标格式写进去。它自己**并不是**又一个记忆系统，没有云端、没有新发明的档案格式；  
它在迁移完只留下一份可验证的回执清单，告诉你都搬迁了什么。

## 设计底线

- **默认不动你的数据。** `plan` 只出报告；真要写到磁盘得 `apply`，
  交互确认。
- **全程本地。** 解析、去重、检测都在你机器上跑，不调用任何远程模型。
- **冲突不替你做主。** 疑似重复只聚类、列成冲突清单，由你裁决。
- **每一步留回执。** 写了什么、从哪来、写完回读是否一致，都落在带 schema 的报告里；  
  下一次运行拿旧回执当依据：迁过的不再重写，你的修改会被保留。
- **密钥检测常开。** 命中必报，默认放行（`init` 时可选改为拦截，选择记进报告）；
  报告里永远不出现命中值本身。

## 安装使用

需要 Rust stable

```sh
cargo build
```

最常见的用法是「家模式」：选一个 OKF 目录当家，其他来源都是卫星，定期往家汇总：

```sh
# 建家（生成 index.md / log.md / .mem-adaptor/，询问密钥策略）
./target/debug/mem-adaptor init ~/my-memory

# 第一颗卫星：某个 Claude Code 项目的记忆
./target/debug/mem-adaptor plan ~/.claude/projects/-Users-me-myproject/memory \
  --to okf:~/my-memory --satellite new --label myproject

# 看过计划报告后写入（plan 结束时会打印报告路径）
./target/debug/mem-adaptor apply <计划报告路径> --yes
```

以后同步不用再带 `--satellite`，直接 `plan` 会认出已登记的卫星：没变的跳过，  
源头改过的原位更新，你和家两头都改过的变成冲突等你裁决。  
直接迁移（源 → UMP JSON，不经过家）把 `--to` 换成 `ump:<目录>` 即可。

退出码是给脚本用的：`0` 正常完成；`1` 输入或 IO 出错；`2` 用法错误；`3` 有迁移被拒/未决定/写完回读不一致；  
`4` 依据过期（比如计划生成后目标又被改过）。

## 现在支持什么（v0.1.0）

|     | 格式                                                                                                                           |
| --- | ---------------------------------------------------------------------------------------------------------------------------- |
| 读   | Markdown 目录（Claude Code `memory/`、Obsidian、OKF 家读回）；ChatGPT 数据导出（`memory.json` 等）；Claude 导出（`memories.json`、`projects.json`） |
| 写   | OKF（Markdown + frontmatter 的家目录）；UMP JSON 数组                                                                                 |


还没做的：PII 检测、Mem0 / basic-memory 写入端、语义级去重（现在是精确去重 + 冲突聚类）、
家往卫星分发。

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

schema 以 `schema/` 下手写的 JSON Schema 为参考，  
配 7 个有效向量和 92 个无效变体。

## 隐私

家目录、计划报告、回执都是敏感数据，别放进公开仓库。工具不上传任何东西；
`lab/` 里的第三方实验代码与真实记忆数据在 git 忽略清单里。
