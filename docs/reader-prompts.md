# 无导出通道产品的记忆抽取 Prompt

**什么时候用这份文档**：源产品的记忆**没有导出通道**，拿不到任何落盘文件。这时 Reader 的
唯一入口就是「让源产品自己把记忆吐出来」。

| 产品 | 有记忆导出通道吗 | 用哪个 Reader |
| --- | --- | --- |
| Claude.ai（网页端） | ✅ 账号数据导出包里的 `memories.json` | 直接解析导出包，**不要**用本 Prompt |
| ChatGPT（网页端） | ❌ 导出包不含记忆（详见 [source-memory-formats.md](source-memory-formats.md)） | **本 Prompt** |
| Gemini（网页端） | ❌ Takeout 只导活动记录，记忆是派生层 | **本 Prompt** |
| 以后任何新产品 | 先查有没有导出；没有就用本 Prompt | **本 Prompt** |

用法：把下面的 Prompt 整段复制，粘贴进那个产品的对话框发出去，把返回的代码块存成
`.md` 文件，交给 Reader 解析。

**当前 D1 CLI 约定**：ChatGPT 的输出请保存为 `*.chatgpt.md`，再把所在目录或 ZIP
交给 `plan`。这是来源标记，不凭通用三列文本猜它来自 ChatGPT 还是 Gemini；
Gemini Reader 尚未实现。只有日期的值会原样保留为自报 metadata，不补造成午夜时间戳。

---

## 1. 输出格式（先定格式，再写 Prompt）

固定三列，一行一条：

```
[日期或 unknown] [类别] 内容
```

| 位置 | 取值 | 为什么这么定 |
| --- | --- | --- |
| 日期 | `YYYY-MM-DD` 或 `unknown` | 迁移报告要 `source_timestamps`；拿不到就显式写 `unknown`，**不许编** |
| 类别 | 5 个固定英文词之一（见下） | 让源产品自己报类型，比我们事后猜准；直接对上 Claude Code 的 `type` 和迁移报告的 `source_layer` |
| 内容 | 原文，不翻译、不总结 | 铁律 6（禁止静默衰减）：内容一旦被总结，就再也拿不回原文 |

类别清单（封闭集合，共 5 个）：

| 类别 | 含义 |
| --- | --- |
| `profile` | 关于我的客观事实：身份、背景、长期情况 |
| `preference` | 我的偏好：喜欢什么、不喜欢什么 |
| `instruction` | 我对你回答方式的要求：语气、格式、语言、「总是…」「永远不要…」 |
| `project` | 我在做的事：项目、主题、反复出现的话题 |
| `tool` | 我用的工具、框架、技术栈、环境 |

**为什么不是 JSON**：长文本里嵌引号和换行，模型很容易吐出不合法 JSON，而且一坏就整份坏。
一行一条的纯文本**坏一行只丢一行**，并且人也能直接读。

**这不是新的交换格式。** 它只是一个「让源产品把记忆倒出来」的临时抽取约定，
不出现在仓库对外的任何格式定义里（铁律 1：不自建交换格式）。Reader 解析完就丢掉这层壳。

---

## 2. 主 Prompt（中文，直接复制这段）

```
请把「你目前记住的关于我的一切」完整导出，供我备份到别处。

只输出一个代码块，每行一条记忆，格式严格固定为：

[日期或 unknown] [类别] 内容

类别只能从这个清单里选一个（写英文单词，不要翻译）：
profile      关于我的客观事实：身份、背景、长期情况
preference   我的偏好：喜欢什么、不喜欢什么
instruction  我对你回答方式的要求：语气、格式、语言、"总是…"、"永远不要…"
project      我在做的事：项目、主题、反复出现的话题
tool         我用的工具、框架、技术栈、环境

日期用你确实知道的，写成 YYYY-MM-DD；不确定或没有就写 unknown，不要编造。

硬性要求：
1. 一条一行。不要编号、不要项目符号、不要分节标题、不要加粗、不要空行。
2. 不要总结、不要合并同类项、不要省略。宁可写多，不要写少。
3. 保持原语言，不要翻译。
4. 除了那个代码块，不要输出任何其他文字，不要反问我。
5. 某一类完全没有就跳过，不要写「无」或任何占位行。
```

## 3. 英文版（产品对英文更听话时用）

```
Export everything you currently remember about me, so I can back it up elsewhere.

Output a single code block only, one memory per line, in exactly this format:

[YYYY-MM-DD or unknown] [category] content

category must be exactly one of these five English words:
profile      objective facts about me: identity, background, long-term situation
preference   what I like or dislike
instruction  how you should answer me: tone, format, language, "always...", "never..."
project      things I am working on: projects, topics, recurring themes
tool         tools, frameworks, stacks, environment I use

Use a date only if you actually know it; otherwise write unknown. Do not invent dates.

Hard rules:
1. One memory per line. No numbering, no bullets, no section headers, no bold, no blank lines.
2. Do not summarize, do not merge similar items, do not omit. Better too many than too few.
3. Keep the original language. Do not translate.
4. Output nothing except that single code block. Do not ask me questions.
5. If a category is empty, skip it. Do not write "none" or any placeholder line.
```

---

## 4. 各家怎么用（差异都在触发方式，不在格式）

| 产品 | 入口 | 需要额外做的 | 已知坑 |
| --- | --- | --- | --- |
| ChatGPT | 直接在对话里发 | 发完再单独问一句「还有吗？把上面漏掉的补上」——它的记忆分两层（显式 saved memories + 自动合成的 summary），一次常只吐一层 | 官方自己承认 summary「不一定包含 ChatGPT 记得的每项细节或来源」，所以**它吐出来的东西不等于它真正记得的**；导出包不含记忆，这是唯一通道 |
| Gemini | 直接在对话里发 | Gemini 没有可编辑的记忆条目清单，记忆是从活动记录里**归纳**出来的。发完补一句「回顾我们过去的对话，把上面漏掉的补上」 | 关掉「保留活动记录」后新对话不进历史，也就不用做记忆了——先确认这个开关是开的 |
| 其他产品 | 直接在对话里发 | 若产品支持生成文件，追加一句「把上面内容存成 memory-export.md」 | 没有导出通道的产品，记忆通常也是服务端合成物，一次拿不全，建议跑两遍取并集 |

**建议跑两遍**：第一次拿到的和第二次拿到的往往不完全一样（合成层会漂移）。
两遍取并集比纠结哪遍「正确」更实在——这本身就是源侧不稳定性的一个实证。

---

## 5. 拿到输出后怎么验收

粘回来之后，**先跑一遍自检再入库**，不要直接相信它是干净的：

- [ ] 整份输出只有**一个代码块**，代码块外没有解释性文字
- [ ] 每行都严格是 `[...] [...] ...` 三段，没有项目符号、编号、标题
- [ ] 类别都落在 5 个词里（出现别的词 → 记进迁移报告的未映射项，不要静默丢）
- [ ] 日期要么是 `YYYY-MM-DD`，要么是 `unknown`（出现别的写法 → 记异常）
- [ ] 没有明显的「合并总结」痕迹（同一件事只剩一条概括 → 说明它没听话，重跑）

一段最小的解析器（照这个形状写进 Reader）：

```python
import re

LINE = re.compile(r'^\[(?P<date>[^\]]+)\]\s*\[(?P<kind>[^\]]+)\]\s*(?P<content>.+)$')
CATEGORIES = {"profile", "preference", "instruction", "project", "tool"}

def parse(text: str) -> tuple[list[dict], list[str]]:
    records, anomalies = [], []
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("```"):
            continue
        m = LINE.match(line)
        if not m:
            anomalies.append(f"line {lineno}: 不符合三列格式 -> {line[:60]!r}")
            continue
        kind = m["kind"].strip()
        if kind not in CATEGORIES:
            anomalies.append(f"line {lineno}: 未知类别 {kind!r}（保留内容，标记未映射）")
        records.append({
            "date": None if m["date"].strip() == "unknown" else m["date"].strip(),
            "kind": kind,
            "content": m["content"].strip(),
        })
    return records, anomalies
```

`anomalies` 就是迁移报告里「源侧不规范」那一栏的原始素材——**不要把它们丢掉**，
它们正是「源系统不设卡」的又一份证据。

---

## 6. 迁移报告里要写清的缺项

用这条通道拿到的记忆，报告里必须显式列出下面几条，否则就是静默衰减：

| 缺项 | 说明 |
| --- | --- |
| `provenance` | **拿不到**。源产品不会告诉你这条记忆是从哪次对话来的（ChatGPT 官方导出里也没有 per-entry 来源） |
| 真实事实时间 | 日期是模型自报的，**不可信**；`unknown` 的比例本身就是质量指标 |
| 被合成掉的细节 | 源侧的记忆已经是「合成层」，原始对话里的细节不在其中 |
| 未导出的条目 | 源产品自己承认清单不完整；拿到的 ≠ 它真正记得的 |
| 原文语言 | 已要求不翻译，但要抽查确认（模型经常偷偷转成英文） |

对应到报告 schema，这几条分别落 `provenance: 无`、`source_timestamps: 不可信`、
`source_unavailable: [...]`——具体字段定义见 [source-memory-formats.md](source-memory-formats.md)。
