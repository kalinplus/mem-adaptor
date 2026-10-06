# 框架图与流程图

四张图对应 [design.md](design.md) 的同一套设计，图里的编号（第 n 阶段、DEC-n）都指向 design.md。
改设计时先改 design.md，再回来同步这里；两边冲突以 design.md 为准。

框里标了交付阶段（D1 / D2–3 / 以后），虚线框是 D1 之后才做的。渲染好的图片在 [diagrams/](diagrams/)。

## 1. 框架图：模块与边界

谁调用谁、谁不许 import 谁。

```mermaid
%%{init: {"flowchart": {"wrappingWidth": 420}}}%%
flowchart TB
    classDef later stroke-dasharray: 5 5,color:#666
    classDef engine fill:#e8f0fe,stroke:#4a6fa5
    classDef artifact fill:#fff4d6,stroke:#b8860b
    classDef independent fill:#eef7ee,stroke:#4a8a4a

    CLI["CLI（主）/ MCP（辅）<br/>config：闸门策略（DEC-1）"]

    SRC["<b>源</b><br/>D1：网页导出 ZIP（ChatGPT / Claude）、本地 Markdown（Obsidian / MEMORY.md / OKF 家）<br/>以后：数据库式 / MCP 服务器（mem0 等）"]

    READERS["<b>Reader 插件</b>（注册表接入）：解析 + 归一<br/>D1：ChatGPT、Claude、Markdown / OKF<br/>以后：社区贡献"]

    subgraph ENGINE["核心引擎（全部本地，不 import 具体适配器）"]
        direction LR
        E1["解压与源识别<br/>第 1 阶段"] --> E2["canonical 校验<br/>+ 字段覆盖校验<br/>DEC-17"] --> E3["密钥/PII 闸门<br/>检测常开，按策略处置<br/>DEC-1"] --> E4["去重与冲突聚类<br/>MVP 只做精确去重<br/>DEC-6"] --> E5["dry-run 截断点<br/>DEC-11"]
    end

    A1["计划报告（预测）<br/>一等 artifact，敏感文件"]:::artifact

    GOV["<b>治理接口</b>（可插拔后端，本仓库不自建）<br/>D1：命令行确认<br/>D2–3：审批钩子（Panella 式哈希链凭证）<br/>以后：快照（Memoria 式 CoW）、审计链、导出授权"]

    E6["引擎写入关卡：重算计划摘要，与审批凭证核对<br/>DEC-3"]:::engine

    WRITERS["<b>Writer 插件</b>（注册表接入）：写入 + 回读验证 + id 桥接<br/>D1：文件式 UMP JSON、文件式 OKF 目录（默认的家）<br/>D2–3：数据库式 Mem0 形状（infer=False）、文件式 basic-memory<br/>以后：图谱式 Graphiti、网页粘贴 Claude / Gemini"]

    A2["回执报告（实际 + 验证结果）<br/>一等 artifact，敏感文件"]:::artifact

    CONF["Conformance 套件（独立）<br/>schema + 测试向量 + 只读执行器<br/>不 import 引擎和适配器，DEC-7"]:::independent

    CLI --> ENGINE
    SRC -- "引擎解压，Reader 逐个认领文件" --> READERS
    READERS -- "源记录 + canonical 记录 + 未承载字段清单" --> ENGINE
    ENGINE -- "默认到此为止" --> A1
    A1 -- "用户显式执行，人确认" --> GOV
    GOV -- "审批凭证" --> E6
    E6 -- "只有引擎能调用写入" --> WRITERS
    WRITERS --> A2
    A2 -. "下一次运行的输入，DEC-18" .-> ENGINE
    CONF -. "只读产物与报告" .-> WRITERS

    class E1,E2,E3,E4,E5 engine
```

要点：

- 引擎和插件之间只通过注册表和数据交接；Reader 不碰闸门，Writer 不能自己决定写不写。
- 闸门在去重之前、在一切出站路径之前，因为去重可能调 embedding，报告本身也会被分享。
- 回执报告指回引擎，就是全部的跨次运行状态，不另建账本。

## 2. 流程图：一次运行

从敲下命令到产出回执，每个分叉点在哪里。

```mermaid
flowchart TD
    classDef stop fill:#fde8e8,stroke:#b94a4a
    classDef artifact fill:#fff4d6,stroke:#b8860b
    classDef human fill:#eef7ee,stroke:#4a8a4a

    START(["mem-adaptor 运行"]) --> P0{"有闸门策略配置？"}
    P0 -- "有" --> PREV
    P0 -- "没有，交互运行" --> ASK["询问策略<br/>默认选项：放行，并说明后果"]:::human
    P0 -- "没有，非交互" --> DEF["按默认放行<br/>标注「来自默认值，未经用户选择」"]
    ASK --> PREV
    DEF --> PREV

    PREV{"有上一次回执报告？"}
    PREV -- "有" --> LOAD["读入：身份映射、content_hash、<br/>验证结果、裁决"]
    PREV -- "没有" --> FIRST["按首次迁移处理<br/>目标已有数据则在计划报告里警告"]
    LOAD --> S1
    FIRST --> S1

    S1["1 解压与源识别<br/>拒绝跳出目录的路径<br/>无人认领的文件进源清单"] --> S2
    S2["2 解析（Reader）<br/>原样字段"] --> S3
    S3["3 归一（Reader）<br/>canonical + 字段映射表 + 未承载字段清单"] --> V
    V{"引擎校验<br/>schema 合规？源字段都有去处？"}
    V -- "有字段两边都没出现" --> VFLAG["引擎标出漏掉的字段<br/>进计划报告"] --> S4
    V -- "通过" --> S4

    S4["4 闸门检测<br/>命中记入 sensitive_findings<br/>只记规则 id + 位置，不记命中值"] --> G{"该级别策略？"}
    G -- "block" --> REJ["rejected"]:::stop
    G -- "pass" --> S5
    REJ --> S6

    S5["5 去重与冲突聚类<br/>精确重复 → omitted duplicate_of<br/>对照上次回执：already_migrated /<br/>deleted_in_target 不重写 / 沿用旧裁决"] --> S6

    S6["6 生成计划报告<br/>逐条预测处置 + 能力声明 + 模型调用预估<br/>+ 闸门策略 + 计划摘要"]:::artifact --> DRY{"用户显式要求执行？"}
    DRY -- "否（默认）" --> END1(["结束：只产出计划报告<br/>目标未被触碰"])
    DRY -- "是" --> S7

    S7["7 人工确认 / 审批<br/>裁决冲突簇、确认 DNA 类 unresolved、<br/>block 模式下逐条放行高危 PII"]:::human --> RCPT["审批凭证<br/>绑定计划摘要"]
    RCPT --> CHK{"写入前重算计划摘要<br/>与凭证一致？"}
    CHK -- "不一致" --> END2(["拒写：批的是计划 A，<br/>现在跑出来的是 B"]):::stop
    CHK -- "一致" --> S8

    S8["8 写入（Writer）<br/>按目标类映射 + id 桥接"] --> S9
    S9["9 回读验证<br/>用目标自己的读接口"] --> RES{"回读结果"}
    RES -- "一致" --> OK["verified"]
    RES -- "不一致" --> MM["mismatch，附差异"]
    RES -- "目标无法回读<br/>如网页粘贴" --> UV["unverifiable"]
    OK --> RR
    MM --> RR
    UV --> RR
    RR["回执报告"]:::artifact --> END3(["结束<br/>回执作为下一次运行的输入"])
```

要点：

- 两个硬截断点都在引擎里：dry-run（默认在这里结束）和计划摘要核对。
- 闸门策略在运行最开始就确定，并被计划摘要锁住；中途改策略要重新出计划报告。
- 人只在第 7 阶段介入一次，所有需要人决定的事（冲突、DNA 类、高危 PII 放行）都集中在这里。

## 3. 家模式：卫星汇总进家

```mermaid
%%{init: {"flowchart": {"wrappingWidth": 520}}}%%
flowchart LR
    classDef later stroke-dasharray: 5 5,color:#666
    classDef home fill:#e8f0fe,stroke:#4a6fa5

    C1["卫星：Claude Code<br/>MEMORY.md"]
    C2["卫星：Codex<br/>本地记忆"]
    C3["卫星：ChatGPT / Claude<br/>网页导出 ZIP"]
    C4["卫星：mem0 等数据库式<br/>（以后）"]:::later

    RUN["<b>一次汇总</b><br/>= 一次目标为家的迁移<br/>闸门 → dry-run → 审批<br/>→ 写入 → 回读"]

    HOME["<b>家：OKF 目录（一个 git 仓库）</b><br/>index.md：okf_version + 按 scope 列表<br/>memories/&lt;canonical_id&gt;.md：正文原文 + 标准字段 + mem_adaptor: 扩展块<br/>log.md：每次汇总一条<br/>.mem-adaptor/config.toml：闸门策略<br/>.mem-adaptor/receipts/&lt;卫星&gt;/：每颗卫星一条回执链"]:::home

    C1 -- "回执链 1" --> RUN
    C2 -- "回执链 2" --> RUN
    C3 -- "回执链 3" --> RUN
    C4 -. "回执链 4" .-> RUN
    RUN -- "写入" --> HOME
    HOME -- "家也是源：OKF Reader 读回，<br/>与卫星新记录一起去重聚类" --> RUN
    HOME -. "家 → 卫星分发（以后再做）" .-> C1
```

要点：

- N 颗卫星只有 N 条回执链，不是两两同步的 N² 条；跨工具冲突在汇总时第一次放到同一张清单上。
- 卫星里消失的记录默认不动家，由人在计划报告里决定 `status: deprecated` 还是移除。
- 回执和配置都存在家目录里，随家走；家目录推到公开远端等于公开全部记忆。

## 4. 单条记录的处置

一条记录在计划报告里落到五态之一，执行后在回执报告里再加一个验证结果。

```mermaid
stateDiagram-v2
    direction LR
    state "闸门" as gate
    state "去重 / 对照上次回执" as dedup
    state "Writer 映射" as mapping
    state "回读" as readback
    [*] --> gate
    gate --> rejected: 策略 block 命中
    gate --> dedup: 未命中 / 策略 pass
    dedup --> omitted: 精确重复 / already_migrated / deleted_in_target
    dedup --> mapping: 新记录或内容变了
    mapping --> unresolved: 冲突未裁决 / DNA 类无法承载
    unresolved --> mapping: 人裁决或确认
    unresolved --> omitted: 裁决落选
    mapping --> accepted: 原样承载
    mapping --> transformed: 承载但被改写，写出改写内容
    mapping --> omitted: 目标不支持，写出原因
    accepted --> readback
    transformed --> readback
    readback --> verified
    readback --> mismatch
    readback --> unverifiable
    rejected --> [*]
    omitted --> [*]
    verified --> [*]
    mismatch --> [*]
    unverifiable --> [*]
```
