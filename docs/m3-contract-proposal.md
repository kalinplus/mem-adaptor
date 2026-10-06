# M3：审批与回执链的契约补充（已确认、已实现）

M2 已通过全部验收；M3 的安全 ZIP 解压、canonical schema 校验和源字段覆盖校验已通过测试。
ZIP 切片目前只防路径穿越和符号链接攻击，尚未设解压资源配额；仍只允许合成测试输入。
下面的推荐路径已于 2026-10-05 经用户确认并实现，对齐 [design.md](design.md) 的 DEC-3、DEC-18。

## 1. 摘要必须绑定正文以外的元数据

M2 摘要中的记录只有 `canonical_id`、`content_hash`、处置预测。
如果正文不变，而 `scope`、`consent`、源 actor 等改变，Writer 的处置可能仍为 accepted，
摘要就不会改变。M2 额外比对源文件 manifest，可以防止正常的源文件变更，
但 manifest 本身不在审批摘要中，不能代替对完整写入含义的绑定。

两个路径：

1. **每条摘要记录加必需 `record_hash`（已选择）**：
   `sha256(JCS(canonical record))`，包含正文与元数据，但报告只存哈希，不内联 canonical 对象。
   Reader 版本、地址、同意信号和源时间因此一起绑定。JCS 摘要外层仍为
   `{records, targets, writers, gate_policy}`，运行时间与运行 id 仍不参与。
2. **只把源 manifest 加进摘要（未选择）**：字段更少，但只绑定源文件；Reader 升版导致的归一结果变化
   仍可能绕过正文摘要，之后还需要补 canonical 元数据哈希。

选择 1 后同步修订 implementation-plan.md、DEC-3 的说明、schema v0 和独立重算测试。
schema v0 尚未发布，直接补充该必需字段，不做兼容分支。

## 2. 防复活不能在一次空写入后丢状态

第一次迁移条目为 verified；第二次是 omitted/already_migrated，不能伪称本次 verified。
如果第二份回执因此抹掉历史写入与验证信息，第三次运行就无法正确判断目标是否被用户删除。

两个路径：

1. **回执 entry 增加 `prior_write`（已选择）**：
   `{target_id, content_hash, record_hash, verification}`，表示此前真实发生的写入及验证；
   历史依据也保留 metadata 哈希，避免正文不变时误判。真实写入时必需，其他条目可选。
   跳过时沿用，实际更新时由新的写入结果替代。`verification` 顶层仍只表示本次回读，
   skipped entry 不写本次 verification。最新一份回执即可独立携带必要状态，不另建账本。
2. **每次沿 `previous_receipt_ref` 回溯所有旧回执（未选择）**：不加子字段，但任何旧文件丢失都会破坏防复活，
   家目录换机器时也必须完整搬运整个历史链。

不自动覆盖用户在目标侧修改的正文：出现双侧变化时输出 unresolved，
增加 `target_modified` 原因。目标已有同名记录、但无可验证旧回执时输出
`target_untracked` 原因，不默默覆盖。
计划条目及摘要中的逐目标预测也带可选 `prior_write`，将覆盖前的已知目标状态绑定审批。
重复条目的历史依据指向实际存活记录；临时从源消失的条目沿回执携带，防复活状态不因空写入丢失。
精确去重只在语义 metadata 一致时折叠空白后比较正文；scope、owner、consent、时间、标签和 DNA 等
不同不会因为文本相同而被自动丢弃。身份与来源路径不参与重复比较，但都保留在逐条报告里。

## 3. 密钥规则来源实验

Gitleaks 官方仓库已检索核实；本地参考在 `lab/upstream/gitleaks/`，只读，不作为运行依赖。
固定参考提交：`b58d3f102cf3a2c84cb7f923d05c25c9b1aed84b`，许可证 MIT，
Copyright (c) 2019 Zachary Rice。

已实际用 Rust `regex` 编译通过的六条：

- `anthropic-api-key`
- `aws-access-token`
- `github-fine-grained-pat`
- `github-pat`
- `openai-api-key`
- `private-key`

已按既定默认复用这些正则，随产品分发其数据和 MIT 署名，另补一个本地 `password=` 赋值规则。
这是**签名正则子集**，不宣称完整实现 Gitleaks 的 entropy、keyword、仓库级 allowlist 机制。
产品自己的规则级白名单由用户显式配置并记入报告，不隐藏命中值或检测事实。

## 验收与边界

测试覆盖正文和未知源字段中的假密钥、pass/block/白名单、跨次跳过、稳定 id 更新、
目标删除、双侧变化、短暂源缺失、重复引用、裁决复用及不可信 canonical 数据拒绝。
Python 用标准库独立重算无数字/非 ASCII 键的实际摘要输入；这不是 M7 完整 RFC 8785 验收器。
ZIP 尚无资源配额，完整 Reader/Writer 与首次配置仍待 M4–M6；不得用于真实记忆。
