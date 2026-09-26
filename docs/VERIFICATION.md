# 验证与验收

## MCP 断线接续与新手教程（2026-09-26）

新增同程序单次调用入口 `agentkanban-mcp --call <工具名> --input-file <JSON文件|->`，与 MCP 共用暂停、参数校验、版本冲突和原始上报记录逻辑。同步规则仅在传输断线时允许主 Agent 使用原程序与数据目录，先精读原 key 再合并正常里程碑；没有新增 MCP 工具、常驻服务或轮询。全局精简规则新增的断线段为 **206 个 Unicode 字符**，不是 token 测量。

全新看板首次打开桌面时生成一个「新手教程」项目、两条教学任务，分别展示进行中与待验收。教程不伪造 Agent 执行时间、用户意见或成果；详情引导补充、验收、退回和归档。新建目录排除纯教程项目，教程不提供交给 Agent 的开工说明；已有用户跳过，归档后重启不会重新生成。

| 检查 | 结果与范围 |
| --- | --- |
| Rust workspace | **88/88 PASS**：核心 43、MCP 27、桌面 18；新增教程 8 项、CLI/进程 7 项 |
| MCP 实际断线 | 结束隔离 stdio 进程后，通过 CLI 精读和版本更新，重启 MCP 后仍是同一任务；覆盖暂停零任务写入、过期版本/错误参数不写入、归档恢复、UTF-8/BOM 文件与标准输入 |
| 教程数据 | 首启恰好两条；旧用户跳过；八线程初始化不重复；反馈/通过/退回；归档重启不恢复；插入失败整体回滚；摘要返回路径可继续精读和归档恢复 |
| 前端回归 | **10/10 PASS**：教程提示、禁止误交接且不调用 handoff、普通任务兼容、新建目录排除教程、同名真实项目和混合项目保留 |
| Windows 原生联动 | **8/8 PASS**：真实 GUI/MCP/CLI 和隔离数据库；首启教程、反馈与验收退回、归档重启、真正 MCP 断线、断线期间人工意见与冲突保护、暂停/恢复、GUI 和 MCP 重启后同 id/进度/步骤/意见；无前端运行错误 |
| 静态与构建 | workspace Clippy、Rust 格式检查、TypeScript/Vite、桌面 debug 构建、差异检查通过 |
| 用户旧示例清理 | 按用户要求归档 `billing-api`、`shop-web`、`stats-lib` 下共 **10 条**旧示例；逐项目查询确认活动记录为零，历史保留，没有删除目录 |

本机证据：`C:\Users\AlexZ\AppData\Local\Temp\agentkanban-recovery-onboarding-20260926`，含 `workspace-test.log`、`desktop-build.log`、`native-recovery.mjs`、`native-results.json`、原生截图和旧示例归档前快照/回执。UI 证据在 `C:\Users\AlexZ\AppData\Local\Temp\agentkanban-tutorial-ui-20260926`；CLI 专项日志在前一轮修复证据目录的 `cli-fallback-tests/`。

原生测试保留了当前安装版进程；隔离首启尝试注册已占用快捷键时返回明确错误并恢复原状态，随后关闭测试进程自身的快捷键与通知。没有替换安装版、修改真实客户端规则或进行付费真实 Agent 断线演练；测试用真实进程中断模拟空档，未等待数小时。CLI 不能替客户端重连，也不能在程序、数据库或本机命令入口全部不可用时记录；两种入口都失败时须保留任务身份并说明未同步，恢复后只合并当前事实。新功能须更新程序并重新加载同步规则后生效。

## 简洁模式（2026-09-26）

按用户确认的布局增加可持久化的简洁模式：保留项目分组，全部活跃任务以「单行标题＋状态」展示，已完成区域仍默认折叠；待验收和已验收明确区分。顶部深浅色旁增加列表切换按钮；长标题省略并保留悬停全文，点击仍能看完整详情。详细模式原有的三条预览和展开偏好保留，窄条模式独立。

- 前端定向回归 **6/6 PASS**：多项目、全部活动任务、完成区、置顶/折叠、模式保存、原详细模式、详情、320 CSS 窄窗与深色。380×520 混合样例的完整可见行数 **2 → 9**，不是所有看板的固定容量。
- Windows 原生回归 **5/5 PASS**：旧设置默认详细、任务列表与状态、详情、进程重启持久化，以及 130% 字号下工具栏/任务状态完整可见、窄条往返保持选择；原生单项目样例完整可见行数 **2 → 8**。
- 新偏好加入后，桌面 Rust **18/18**（含旧设置兼容）与 Clippy 通过；最终 TypeScript/Vite、桌面 debug 构建及差异检查通过。文档本地链接 **42/42** 可定位。

原生证据为下方修复证据目录内 `native-concise-results.json` 和对应 `concise-native-*/` 截图；前端定向证据为 `C:\Users\AlexZ\AppData\Local\Temp\agentkanban-concise-ui-20260926/results.json`。本功能和以下修复均尚未覆盖安装中的旧版本，未进行人工日常体验验收。

## 修复回归（2026-09-26）

本轮覆盖审查发现的 25 项问题，重点为 MCP 协作、上下文与任务接续。保留原有工作区改动；以下为当前源码和隔离构建的验证结果，没有替换已安装应用或真实客户端配置。

| 审查项 | 修复与验证范围 |
| --- | --- |
| 1–2：Hermes 嵌套 YAML、Claude 接入失败丢配置 | Hermes 仅替换直属服务器，遇不支持的复杂写法拒绝且不写入；Claude 改为保留其他字段的 JSON 合并，先验证、备份，再写入 |
| 3–7：更新目录前缀、接入误判、DSH 重复、CRLF 旧规则、SkipBuild | 安装目录精确匹配；检查启动参数、环境及启停；拒绝未托管的同名 DSH 条目；三种旧规则兼容 LF/CRLF；更新失败恢复原浮窗，SkipBuild 不依赖 target |
| 8：旧版 done 自动归档 | 仅自动归档人工已验收的旧任务；迁移得到的 done/none 保留 |
| 9–11：设置丢失、旧回包覆盖新输入、连续操作被吞 | 保存队列移入 App，合并待保存字段、串行写入、维持最新显示；关闭面板或切页不丢失输入 |
| 12：窄窗与 130% 字号裁切 | 原生最小尺寸随缩放调整；真实 WebView 的 130% 界面保持 320×360 CSS 像素，窄条仍可展开 |
| 13–14：暂停读取重试与恢复语义 | 重试同时重读暂停状态；暂停不轮询，下个正常里程碑或新任务尝试一次，同一 MCP 进程可恢复 |
| 15–17：旧退回意见、通过后备注隐藏、复制失败 | 新一轮验收清空自动带入的旧意见，保留主动草稿；显示已验收备注；开工说明可展开并手动复制 |
| 18：同秒备份覆盖 | 纳秒、进程号与序号组成独立文件名；连续备份分别恢复出不同完整快照 |
| 19–20：原始需求不可见、完成/归档任务接续 | 摘要增加 has_request；精确 task_key 默认找回完成/归档记录，显式过滤仍有效 |
| 21：无版本并发覆盖 | 已有任务实质写入/归档必须带版本；两个进程同时创建不同内容时只有一个成功；冲突零写入，精读后合并 |
| 22–23：全页扫描和全量步骤反复传递 | query 定向查询，摘要路径去重与进展截断；step_updates 按索引增量改状态/备注；同一任务一个记账 Agent，子 Agent 返回结果 |
| 24–25：文档与原始上报语义 | 当前文档同步 schema 5、最近 30 次历史；新 MCP 保存实际提供字段，保留省略/null 差别，旧历史不改写；界面称「Agent 上报记录」并正确显示增量步骤 |

| 检查 | 当前结果 |
| --- | --- |
| Rust workspace | **73/73 PASS**，包含真实独立进程并发创建、版本竞争、暂停、迁移、事务回滚、步骤增量与备份 |
| 静态与构建 | `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`、`cargo fmt --all --check`、TypeScript/Vite 与桌面 debug 构建通过 |
| 独立 MCP 黑盒 | **18/18 PASS**；新 schema 仍只有三个工具 |
| 界面回归 | **17/17 PASS**（含一个无模拟浏览器预览），追加上报记录定向检查 **2/2 PASS**；交互使用真实前端和隔离模拟的 Tauri 桥 |
| Windows 原生联动 | **8/8 PASS**：真实 Tauri WebView、真实 MCP、独立 SQLite；涵盖新建需求、设置持久化、缩放/窄条、人工反馈冲突、步骤历史、暂停恢复、验收/归档和进程重启；无 pageerror |
| 客户端配置 | 桌面 workspace 内 **9 项**隔离配置回归通过；未向真实客户端写入 |
| 更新脚本 | **8/8 PASS**，使用临时文件和进程/安装器模拟，含兄弟目录保护、无 target、预检查、失败恢复与占用阻断 |

上下文对照使用安装目录旧 MCP 与本轮新 MCP 的固定快照、相同的 121 字符临时项目路径；只统计请求及响应 `content`，不重复计算 `structuredContent`。**35/35 断言通过**，最终计划与备注一致：

| 场景 | 调用次数：旧 → 新 | 请求字符：旧 → 新 | 返回字符：旧 → 新 |
| --- | --- | --- | --- |
| 完整遍历 50 条摘要 | 10 → 10 | 2,637 → 2,637 | 17,778 → 11,858 |
| 唯一关键词发现＋精读 | 11 → 2 | 2,916 → 548 | 18,420 → 1,027 |
| 最大 12 步计划，更新 12 次 | 12 → 12 | 56,612 → 5,338 | 857 → 857 |
| 短 3 步，1 次 list＋3 次 upsert | 4 → 4 | 1,950 → 1,741 | 237 → 385 |

长计划更新请求减少 90.6% 字符，短流程请求与返回合计仅减少 2.8%。工具 schema 固定体积从 4,427 增至 5,296 字符（+869）；完整精读未缩减。上述为字符/UTF-8 字节实测，**不是模型 token、总上下文长度或账单测量**；没有新增付费模型评测。新规则减少全页读取、重复步骤和多人重复记账，但真实 Agent 的调用习惯仍须在升级后的会话中观察。

完整脚本、日志和截图位于本机 `C:\Users\AlexZ\AppData\Local\Temp\agentkanban-repair-20260926-145531`，其中 `native-results.json`、`native-layout.json`、`mcp-final.json` 与 `context-benchmark/run-20260926T071308887245Z/summary.json` 为汇总。界面模拟证据位于 `C:\Users\AlexZ\AppData\Local\Temp\agentkanban-ui-regression-20260926`。失败证据保留：首次原生探测误用了 Tauri 从旧侧车复制来的 MCP；重新执行 `prepare:mcp` 后固定新产物并验证 schema，再执行正式对照。后续一次选择器歧义修正后通过，不计作产品失败。动态 zoom 后 Playwright 默认截图会裁小捕获区域，另以 CDP 原尺寸截图与原生 inner_size 核实窗口为 624×702 物理像素（系统缩放 1.5），控件均在视口内。

当前未执行真实升级、跨客户端真实 Agent 工作流复测或人工日常体验验收。已安装的旧 MCP 仍在运行；本轮改动需要后续部署并重启客户端后才能在日常会话中生效。以下旧版本记录属于历史证据，不等同于本轮已复测。

## v0.4 实测结果（2026-09-23）

本轮把入板方式改为「会修改文件的任务自动记录」，新增计划步骤 `steps`、`task_list` 摘要模式（默认 5 条）、浮窗归档、「已撤回验收」痕迹、数据库命令异步化，并修复多个 MCP 进程同时首次打开数据库时偶发 `database is locked` 退出的问题。

| 检查 | 结果与范围 |
| --- | --- |
| Rust 自动检查 | PASS：48 项（数据 25、MCP 15、桌面 8），含 schema 2→3 迁移、`steps` 省略保留与校验、撤回验收痕迹、浮窗归档不计 Agent 更新、摘要与完整记录两种查询、工具说明体积上限 |
| 并发启动 | 修复前 HEAD 在 9 次重复中失败 1 次，本地复现为 `Database error: database is locked`；加入 5 秒内的有界重试后连续 20 次 PASS |
| 独立 MCP 进程 | PASS：debug 18/18、[release 18/18](evidence/v04/mcp-release.json)（默认分页改为 5 条并遍历全部页面） |
| Windows 交付与接入 | PASS：0.4.0 NSIS 与免安装 ZIP 构建；静默安装到 `%LOCALAPPDATA%\AgentKanban`，GUI 启动并创建数据库；用户 Codex 配置接入后，真实会话只读调用 `task_list` 成功。安装向导界面、卸载未执行 |
| 前端 | PASS：TypeScript 与 Vite 构建。桌面 WebView 与 Windows 现场交互本轮 NOT_RUN |
| 真实 Codex 行为 | PASS：[结果](evidence/v04/agent-eval-results.json)、[多步骤任务记录](evidence/v04/codex-multi-with-board.jsonl)、[评测脚本](evidence/v04/agent-eval-run.mjs)。4 个编码任务各生成 1 条独立任务，每个都是 1 次 `task_list` + 3 次 `task_upsert`（开工含计划、一次阶段更新、完成），全部携带 `expected_updated_at`，无冲突、无重复任务；新会话的新功能新建任务而非覆盖旧任务；只读问题和明确「不用记」均 0 次调用；最终回复除「未记录到看板」一句外不提看板 |
| 本机更新 | PASS：`npm run desktop:update` 完整构建并覆盖安装；浮窗运行时关闭并在安装后重启；MCP 被占用时在任何改动前停止并列出 PID，浮窗不受影响；`-StopMcp` 结束占用进程后完成安装。安装版本与构建一致的检查只比对 MCP（Tauri 打包后会重写 `agentkanban.exe`） |
| 暂停记录 | PASS：协议与跨进程测试覆盖暂停后三个工具零写入、对已初始化会话生效、恢复后照常。[真实 Codex](evidence/v04/pause-eval-results.json)：会话前已暂停时仅 1 次 `task_list` 收到暂停回复，之后不再调用，任务正常完成，看板 0 条，总输入比未接看板基线约多 3.5k token；会话中途暂停时，下一次 `task_upsert` 收到暂停回复后不再调用，任务正常完成，看板停留在暂停前最后一次上报的「进行中」。另确认 Codex 支持 `mcp_servers.<name>.enabled=false`，但 AGENTS.md 规则仍在上下文中，未采用该方式 |
| Codex 规则投递 | 实测 Codex CLI 0.156.1 向模型只提供 MCP 工具名：[服务器说明探测](evidence/v04/probe-instructions.jsonl)、[工具描述探测](evidence/v04/probe-description.jsonl)。仅靠 MCP 时 0 次入板；在 AGENTS.md 加一段规则（[片段](../examples/codex-AGENTS-snippet.md)，约 150 token）后生效 |

Token 与上下文（Codex CLI 0.156.1，`gpt-6-astra` high，同一提示、临时项目，基线不接 MCP、不加规则；两组执行的命令与文件修改数相同）：

| 场景 | 接入看板 输入 / 其中缓存 / 输出 | 基线 输入 / 其中缓存 / 输出 | 差值 |
| --- | --- | --- | --- |
| 小改动（2 次平均） | 113,304 / 102,336 / 1,207 | 70,072 / 56,896 / 692 | 输入 +43k（+62%，多为缓存命中），输出 +515 |
| 多步骤工具 | 120,924 / 110,208 / 3,470 | 93,508 / 84,992 / 2,869 | 输入 +27k（+29%），输出 +601 |

每个任务的看板调用参数与返回合计约 2.0–2.3k 字符（约 0.7–0.9k token），这是实际留在对话上下文里的部分。输入 token 增量主要来自 4 次工具调用各多一轮模型请求、重复读取已缓存的上下文，而不是看板内容本身。每次单样本存在网络与缓存波动，数值用于量级判断。

仍未验证：Claude Code、Cursor 下的自动入板行为；更长任务中的更新频率；Windows 物理交互与安装包。

## v0.3 实测结果（2026-09-22）

本轮补齐文字新建、`Ctrl+Alt+N`、开工说明、Agent 接手信息、下一步、所需输入、交付物、用户补充与人工验收。四种任务状态不变，新增独立验收状态；语音与自动派发未实现。

| 检查 | 结果与范围 |
| --- | --- |
| Rust 自动检查 | PASS：41 项（数据与 MCP 33 项，桌面 8 项）。包含 schema 1→2 迁移与失败回滚、旧数据和设置保留、创建幂等、退回→重做→再交付→验收、无变化不撤销验收、并发版本冲突零写入；Clippy `--all-targets -- -D warnings` 与工作区格式检查通过 |
| 前端生产构建 | PASS：TypeScript 与 Vite 构建；沿用既有 React/Tauri 架构 |
| 独立 MCP 进程 | PASS：[debug 18/18](evidence/v03/mcp-debug.json)、[release 18/18](evidence/v03/mcp-release.json)。仅三个工具；新增字段往返、精确查询、省略保留/显式清空、过期更新拒绝、禁止 MCP 写人工字段均通过 |
| 真实桌面 WebView | PASS：[10 项](evidence/v03/ui.json)。文字新建、错误提示、草稿保留、系统剪贴板实际内容、Agent 归属与所需输入、人工反馈不刷新 Agent 时间、成果展示与非 HTTP(S) 拒绝、并发验收保护、退回原记录、再次交付与验收、GUI 退出后 MCP 写入及重启恢复。含页面错误检查 |
| 输入与异步竞态 | PASS：[6 项](evidence/v03/input-races.json)。创建期间锁定编辑与关闭；未提交反馈在快捷新建后保留，且保留原版本校验；反馈/验收保存期间防止面板被快捷键替换；延迟返回的旧设置不会覆盖快速新建事件中的展开状态；含控制台健康检查 |
| 快捷键与布局 | PASS（已列范围）：两个全局快捷键的实际注册/停用、窗口内 `Ctrl+N`、`Ctrl+Enter`、Escape，以及 `quick-create` 原生事件载荷；浅色/深色、长中文、320×360/380×520/480×640 WebView 视口无横向溢出。系统实际缩放 150%，其余为 CDP 模拟 |
| 真实 Codex 文字审阅任务 | PASS：[两会话结果](evidence/v03/agent-review-results.json)、[第一会话工具记录](evidence/v03/codex-v03-review-first.jsonl)、[第二会话工具记录](evidence/v03/codex-v03-review-second.jsonl)。GUI 创建→Codex 查询/接手/交付→GUI 退回→新会话读取修改意见/重新交付→GUI 验收；始终同一条记录，每会话 4 次真实 MCP 调用。报告由真实 Agent 生成，验收按钮由测试控制器操作，不代表用户日常验收 |
| 真实 Codex 编码任务 | BLOCKED：[实际记录](evidence/v03/codex-coding-blocked.jsonl)。自动审批策略拒绝目录检查与 `node --version`，返回 `blocked by policy`。Agent 如实写回 `blocked`、下一步及所需环境，没有创建代码或运行测试，也没有通过更改权限重试 |
| Windows 交付 | PASS：0.3.0 NSIS x64 与免安装 ZIP 构建；[release 启动记录](evidence/v03/release-startup.json)确认版本、窗口响应、SQLite 和数据目录下的 WebView 缓存，stderr 为空。安装/卸载向导未执行 |
| 仍未现场验证 | Windows 物理全局按键触发、托盘点击、鼠标拖动/边缘缩放、快捷键被其他应用占用时的实测、重新登录启动、Claude Code/Cursor 连接、用户日常使用验收均 NOT_RUN。登录启动注册项开关的 v0.2 实测保留在下方，本轮未重复 |

Codex CLI 0.155.1 使用已有 ChatGPT 登录，测试沿用当前 `gpt-6-astra` 与 `max` 设置。MCP 仅以临时命令行配置连接隔离数据库，没有修改用户客户端配置、安装全局 Skill，或往日常看板写测试任务。编码演示的受阻记录与后续文字审阅使用不同任务/数据目录；文字审阅成功不代表编码执行问题已解决。

Browser plugin 未提供，使用既有 Playwright 通过 debug WebView2 CDP 检查真实 Tauri 窗口。输入竞态检查只对当前测试 WebView 的 bridge 网络响应增加延迟，不伪造数据库结果；所有写入和 MCP 更新仍走真实接口。本轮未再尝试全局物理按键与托盘输入；此前 Windows Computer Use 曾报 `native pipe unavailable`，事件测试没有记作物理按键通过。

本机完整测试脚本、截图与原始失败记录保存在 `%TEMP%\agentkanban-v03-20260922\`。其中保留了测试脚本的剪贴板读取方式/Windows 换行、默认三条预览、启动等待与定位范围修正记录；反馈输入框补充了固定的可访问名称，避免有初始内容时名称混入正文。最终发布版不含测试注入代码。

0.3 升级会将数据库迁移到 schema 2。先关闭旧 GUI 与旧 MCP 进程、备份完整数据目录，再更新两个程序；不能用旧版程序打开迁移后的库。以下保留 v0.2/v0.1 历史实测，不将旧版只读界面规则误作当前功能限制。

## v0.2 实测结果（2026-09-22）

本轮新增接入诊断、实际路径配置复制、本地 MCP 自检、久未更新提示、项目聚焦与置顶、只读详情、可选登录启动及全局快捷键。保留下方 v0.1 原始结果。

| 检查 | 结果与范围 |
| --- | --- |
| Rust 自动检查 | PASS：25 项测试（12 数据、8 MCP、5 桌面）；桌面和数据模块 Clippy `--all-targets -- -D warnings`、全工作区格式检查通过 |
| 前端生产构建 | PASS：TypeScript 类型检查与 Vite 构建 |
| MCP debug / release 真实进程 | PASS：各 12/12；协议仍只有 3 个工具；不依赖运行中的 GUI |
| 真实桌面 WebView 主流程 | PASS：11 项。旧偏好兼容、项目置顶排序与聚焦、状态筛选、更新不重置视图、提示阈值开关、详情全文与复制、Escape 焦点恢复、诊断与配置复制、真实 MCP 自检、隐藏恢复、重启保留偏好；无 WebView 页面错误 |
| 边界行为 | PASS：4 项。聚焦项目全部归档后明确显示空项目；恢复任务不重开详情；快照读取失败保留已读偏好；偏好读取失败时禁用修改，重试后恢复。最后一项检测还确认可见时有实际读取、隐藏时读取停止、恢复立即读取 |
| 登录启动 | PASS（注册项开关）：通过设置页实际启用并关闭，读取系统实际状态确认，最终恢复测试前关闭状态；没有注销或重启 Windows，实际登录启动 NOT_RUN |
| 全局快捷键 | PASS（原生注册）：实际停用/注册 `Ctrl+Alt+K` 并检查持久化。物理按键触发 NOT_RUN：Windows Computer Use 重置重试仍报 `native pipe unavailable` |
| 布局 | PASS：浅色/深色、长中文标题、长进展及路径；320×360、380×520、480×640 视口无横向溢出。当前系统实际缩放 150%；其他缩放为 CDP 模拟 |
| 安装包及免安装包 | PASS：0.2.0 NSIS x64 与 ZIP 构建；包含两程序、同步规则及可选 Skill。实际运行免安装 GUI：版本 0.2.0、窗口响应、数据库及其下 WebView 缓存创建正常、stderr 为空 |
| 客户端及用户验收 | v0.2 未重新调用模型执行 Agent 演示；v0.1 Codex 实测保留在下方。Claude / Cursor、安装卸载向导、物理托盘与拖动、Windows 重新登录、持续日常使用仍未验证 |

测试使用独立临时数据目录；没有更改已有客户端配置、安装全局 Skill，或在日常数据库创建验收样例。Browser plugin 未提供，使用 Playwright 通过调试版 WebView2 的 CDP 检查真实 Tauri 窗口，release 仅做启动检查。

本机证据保存在 `%TEMP%\agentkanban-v02-20260922\`：`results.json`（11 项主流程）、`edge-results.json`（4 项边界检查）、`mcp-debug.json`、`mcp-release.json`、`release-startup.json` 与 PNG 截图。原始失败结果保留为 `*-before-*.json`；其中包括已修复的模态框焦点问题，以及测试脚本对异步复选框、TOML 末尾换行和只读内部接口的错误假设。

异常读取通过测试专用网络拦截在前端 bridge 注入失败，不改变发布代码。最终的隐藏读取检查在 bridge 中计数，并先证明可见时计数增长，避免把未生效的内部接口替换误判为通过。MCP 自检检查前后数据库 revision 相同，没有创建或修改任务。

## v0.1 验证与验收

记录必须区分实现、自动验证、客户端连接、原生窗口行为和用户验收。脚本通过只证明其覆盖的接口行为；浏览器截图不能证明原生窗口置顶或托盘功能。

## 自动验证

在仓库根目录运行构建脚本，得到真实 MCP 可执行文件后执行：

```powershell
node .\scripts\verify-mcp.mjs .\target\release\agentkanban-mcp.exe --report .\artifacts\verification\mcp.json
```

验证脚本不加载聊天历史、不调用模型、不修改客户端配置。它在系统临时目录下创建独立项目与数据目录，通过 `AGENTKANBAN_DATA_DIR` 传给每个子进程。

| 检查 | 判定依据 |
| --- | --- |
| MCP 协议 | `initialize`、`notifications/initialized`、`ping`、`tools/list` 和工具定义 |
| 状态流转 | 创建 → 进行中 → 受阻 → 完成 → 重新打开，身份保持不变 |
| 幂等性与中文 | 重复更新只有一条记录，中文标题、进展和分支往返一致 |
| 归档恢复 | 默认隐藏归档项；扩展查询可见；恢复后保持原身份 |
| 分页与过滤 | 默认最多 20 项、显式分页、项目过滤和已完成过滤 |
| 持久化 | 关闭 MCP 后新进程可读到原记录，全过程不启动 GUI |
| 并发 | 四个独立 MCP 进程写入 32 条不同任务，无遗漏或重复身份 |
| 错误 | 无效参数、未知方法/工具、不存在任务、已连接后的数据库写入失败和不可用数据目录明确失败 |
| 兼容与输出 | 较早协议握手；标准输出保持 JSON-RPC，不混入普通日志 |

成功后清理脚本创建的临时数据，传 `--keep` 可保留；失败时自动保留数据与 `verification.json`，控制台给出路径。`--report` 另外保存机器可读结果。缺少可执行文件时先构建，不应以模拟服务代替真实服务。

## Windows 窗口现场检查

使用独立数据目录启动 GUI 与 MCP，避免把验收样例混入日常看板。逐项记录实际观察结果：

1. 首次浅色、约 380 × 520；窗口可以拖动和缩放，显示缩放 100% / 125% / 150% 下文字与按钮不裁切。
2. 置顶开关分别开启和关闭，在另一应用窗口前后检查实际层级。
3. 切换深色主题；关闭并重启程序后，主题、位置、尺寸及展开状态恢复。
4. 收成窄条后显示进行中/受阻数量；再展开恢复正常窗口尺寸。
5. 关闭按钮只隐藏窗口，托盘仍存在；托盘恢复显示，托盘退出结束 GUI。
6. 隐藏或退出 GUI 后从 MCP 更新任务；重新显示或启动 GUI，立即看到最新结果。
7. 多项目、长中文标题和大量任务下，内容在窗口内滚动；每项目默认三条未完成项，已完成默认折叠；任务更新不强制展开。
8. 浮窗只提供查看、筛选和折叠等操作，没有任务编辑输入框。

## 客户端与真实 Agent 流程

对每个实际可用的客户端分别记录版本、配置路径范围、连接结果和工具调用结果。读取配置或执行 JSON/TOML 语法检查属于配置检查，不能标作客户端连接通过。

1. 用独立测试配置连接真实 `agentkanban-mcp.exe`，确认三个工具可用。
2. 对 Agent 说「把验收演示功能加入看板，后续同步进展」，确认它创建稳定 `task_key`。
3. 让 Agent 报告一个真实阶段变化，确认同一条记录更新为进行中。
4. 完成演示工作后由 Agent 标成已完成，确认它进入已完成区且默认未完成查询不再返回它。
5. 新会话查询该项目并确认能定位同一条任务；需要复验时重新打开原任务。

只有真实 Agent 使用已连接工具完成上述过程，才能标作 Agent 流程通过。手工 JSON-RPC 脚本只证明协议和数据行为。未安装、未登录或未执行的客户端保留 `NOT_RUN` 并说明具体原因。

## 本次结果

2026-09-22 在 Windows 上实际执行。所有样例使用临时数据目录，没有把验收任务写入日常数据库，也没有更改已有客户端配置。

| 项目 | 状态 | 证据/缺口 |
| --- | --- | --- |
| Rust 数据模块与 MCP 自动检查 | PASS | `cargo test -p kanban-core -p agentkanban-mcp`：19 项通过（11 数据、5 协议、3 子进程）；含 worktree 归并、锁等待/超时、4 进程 48 任务 96 次写入。两个模块及桌面 `clippy --all-targets -- -D warnings` 均通过 |
| 前端类型检查与生产构建 | PASS | `npm run build`：TypeScript 与 Vite 生产构建通过 |
| MCP 真实进程黑盒检查 | PASS | [debug 12/12](evidence/mcp-debug.json)、[release 12/12](evidence/mcp-release.json) |
| Windows 安装包构建与文件完整性 | PASS | NSIS x64 安装包与 ZIP 已生成；生成的 installer.nsi 与 ZIP 内容均包含 GUI、MCP、README、docs、examples 和配置生成脚本。未在用户系统执行安装/卸载向导 |
| release 桌面启动 | PASS（启动检查） | [进程与窗口记录](evidence/release-startup.json)：实际启动免安装目录中的 GUI，窗口响应、SQLite 与 WebView 数据目录创建正常；不等同于 release DOM 自动验收 |
| 真实 Tauri WebView 与原生 API 检查 | PASS（已列范围） | [8 项交互](evidence/ui-results.json)、[重启恢复](evidence/persistence-results.json)、[新增 5 项目 80 任务布局](evidence/many-results.json)。主题、原生置顶 getter、48px 窄条、筛选/折叠、隐藏停止查询、MCP 独立写入、第二次启动恢复、重启持久化通过 |
| 托盘菜单点击、鼠标拖动/边缘缩放、系统缩放切换 | NOT_RUN | Windows Computer Use 重试并重置后仍报 native pipe unavailable。已验证关闭按钮确实隐藏，第二次启动可恢复；没有宣称实际点过托盘。系统实际为 125%，100%/150% 仅 CDP 模拟 |
| Codex 真实连接与 Agent 流程 | PASS | Codex CLI 0.155.1，ChatGPT 登录，临时 CLI 配置；[首会话](evidence/codex-demo.jsonl) todo → in_progress → blocked，[后续会话](evidence/codex-resume.jsonl) 查询原记录 → in_progress → done，始终 id=1 |
| Claude Code 真实连接与 Agent 流程 | 连接 NOT_RUN；Agent 流程 NOT_RUN | 2.1.278 隔离配置检查通过；user scope 注册被自动审批拒绝，安全改用临时 project scope 后显示 Pending approval，尚未连接。[实际记录](evidence/claude-connection.txt) |
| Cursor 真实连接与 Agent 流程 | NOT_RUN | PATH 未找到 `cursor` / `cursor-agent`；未运行连接与 Agent 流程 |
| 用户日常使用体验验收 | NOT_RUN | 交付后由用户体验确认 |

Codex 首次尝试只读查看报告时，CLI 执行策略返回 `blocked by policy`。实际 Agent 将其上报为受阻。主 Agent 随后把已经生成的报告原文作为下一会话输入，避免额外文件访问；新会话核对 12 项结果并通过 MCP 完成原任务。此处不是手工写数据库或脚本模拟 Agent 工具调用。

视觉方法、概念比较、视口和明确差异见 [视觉核对](design/REVIEW.md)。已修复 Vite 监听 Rust 构建目录导致的 Windows 文件锁退出、窄条切换重复加锁风险、窗口几何设置并发落盘顺序问题，以及 WebView 缓存目录未跟随数据目录的问题。最后一项修复后的实际 release 启动记录确认 `webview_under_data_dir: true`，旧观察保留在 `release-startup-before-cache-fix.json`。
