# AgentKanban 发布文案

- GitHub：https://github.com/keviccz/AgentKanban
- 下载：https://github.com/keviccz/AgentKanban/releases/latest

## 发之前

- **先发一个新版本**。桌宠目前只在 main 分支上，最新的 Release 还是 0.7.0。下面的文案都提到了桌宠，发帖前要先发 0.8.0，否则别人下载到的版本里没有。
- **小红书、抖音会对站外链接限流**。正文里写「GitHub 搜 AgentKanban」，链接放评论区或个人主页。
- **素材在哪**
  - 视频：`E:\A_Tools\remotion\out\`。
    - 带桌宠的 34 秒版本：中文横版 `agentkanban-promo-pets.mp4`、英文横版 `agentkanban-promo-en-pets.mp4`、中文竖版 `agentkanban-promo-vertical-pets.mp4`。
    - 旧的 28 秒版本（没有桌宠）还在同一目录，文件名不带 `-pets`。
  - 截图和动图：`docs/images/`，包括 `hero-zh.png`、`hero-en.png`、`pet-zh.gif`、`pet-en.gif`。
  - 封面：`launch-covers/`，每个平台一张。
- **三个版本都已加上桌宠段落**。另外可以录一段 10～15 秒的桌宠屏幕录像（拖动、右键开重力模式、掉到任务栏上溜达），适合抖音、小红书和 X。

---

## 小红书

**封面**：`launch-covers/xiaohongshu.png`（3:4）

**配图顺序**：
1. 封面
2. `hero-zh.png`
3. 桌宠录屏（小红书图文里 GIF 不会动，用视频或实况图发）
4. 设置页截图

**标题**：同时开 3 个 AI 写代码，我给它们做了个看板

**正文**：

最近写代码基本是 Claude Code 和 Codex 一起开，开多了就乱：谁在做哪个需求、做到第几步、哪个卡住在等我回话，全靠脑子记。

所以做了 AgentKanban，一个挂在桌面角落的小看板：

🟢 Agent 自己登记任务、写计划、报进度，我不用手动记
🟠 只有卡住了、需要我补充的时候才弹通知
✅ 做完的任务直接变灰，右键点一下就算验收
👾 顺手做了像素桌宠：每个 Agent 一个小人站在看板上，干活时推箱子，需要你就举个「?」。可以拖到桌面上，开了重力模式还会掉到任务栏上溜达

免费开源，数据只存在本机，不用注册，也不调用任何模型 API。支持 Claude Code、Codex、Cursor 等 7 个客户端，一键接入。目前只有 Windows 版。

GitHub 搜 AgentKanban 就能找到 👀

#AI编程 #ClaudeCode #Codex #Cursor #程序员 #效率工具 #开源项目 #桌面美化 #独立开发

---

## 抖音

**封面**：`launch-covers/douyin.png`（9:16）
**视频**：`agentkanban-promo-vertical-pets.mp4`

**文案**：

同时开三个 Coding Agent，没过多久就记不清谁在干嘛、干到哪了。

所以做了个桌面小看板：Agent 自己登记任务、报进度，卡住了才来找我，做完我点一下验收就行。还配了像素桌宠，哪个 Agent 在干活、哪个在等你，看一眼小人就知道。

免费开源，数据只存在本机。GitHub 搜 AgentKanban。

#AI编程 #ClaudeCode #Codex #Cursor #程序员 #开源工具 #桌宠

> 置顶评论：github.com/keviccz/AgentKanban （目前只支持 Windows）

---

## 知乎

两种发法：发一篇文章，再用文章内容去回答相关问题。

**题图**：`launch-covers/zhihu.png`（16:9）

**文章标题**：我做了一个让 Coding Agent 自己记账的桌面看板（开源）

**正文**：

同时开几个 Coding Agent 以后，我发现最累的已经不是写代码了，而是记住这些事：哪个 Agent 在做哪件事，做到了第几步，哪件事其实卡在等我回话。聊天窗口一多，这些信息就散落在各个终端里。

于是写了 AgentKanban。它是一个常驻桌面的小浮窗，思路很简单：让 Agent 自己记账。

**它怎么工作**

AgentKanban 自带一个本地 MCP 服务。Agent 开始一件会改文件的任务时，会自己调用 `task_upsert` 登记目标、验收标准和计划步骤；之后只在某一步完成、真的卡住或者全部做完时才更新。看板从本地 SQLite 读数据，随时显示最新状态。

（插图：`hero-zh.png`）

我比较在意这几个设计：

- **不打扰**：只有「受阻」和「需要你补充」会弹通知。完成的任务直接变灰，要不要验收你自己决定。
- **省上下文**：MCP 只有 3 个工具，写入只返回约 30 token 的回执，查询默认只给 5 条摘要。记账不该挤占 Agent 干活用的上下文。
- **不刷屏**：同步规则写在工具说明和全局指令里，Agent 只在阶段节点更新，不会每跑一条命令就报一次。
- **全在本机**：不调用模型 API，不上传数据，不用注册账号。数据就放在 `%LOCALAPPDATA%\AgentKanban`。

**像素桌宠**

最近加了一个不太「正经」的功能：每个 Agent 一个像素小人，站在看板顶上把状态演出来。推进中推着写有步骤数的箱子，需要你时屏幕上冒出「?」，完成了亮绿勾，验收时跳一下。小人可以拖到桌面任何地方，点一下直接打开它的任务。右键开启重力模式，它们会掉到任务栏上随机溜达，有事找你时再飞回看板。

本来只是觉得好玩，用下来发现瞄一眼小人就知道哪个 Agent 在等我，比看列表还快。

（插图：`pet-zh.gif`，知乎支持 GIF）

**怎么用**

1. 从 GitHub Releases 下载安装。支持 Windows 10/11，不需要管理员权限。
2. 首次打开点「一键接入全部」。支持 Claude Code、Codex、Cursor、OpenCode、Antigravity、DeepSeek Harness 和 Hermes。
3. 重启客户端，让 Agent 做一件会改文件的小事，看它出现在看板上。

目前只有 Windows 版，MIT 开源。欢迎提 issue，也想听听大家同时开多个 Agent 时都是怎么管理的。

GitHub：https://github.com/keviccz/AgentKanban

**适合去回答的问题**：在知乎上搜这几组关键词，挑近期热度高的问题来回答，正文用上面文章的前半部分加桌宠那段：
- 「同时用多个 AI 编程工具」
- 「Claude Code 技巧 / MCP 推荐」
- 「好用的 MCP Server」
- 「AI 编程 任务管理」

---

## X

**主帖**（附 `agentkanban-promo-en-pets.mp4`）：

I kept losing track of which coding agent was doing what, so I built AgentKanban.

Agents log their own tasks, plans and progress on a small desktop board. They ping you only when stuck, and wait for your sign-off.

Free, open source, runs locally.
github.com/keviccz/AgentKanban

**回复 1**（附 `pet-en.gif`）：

It also ships with pixel desktop pets, one per agent, standing on the board. They push a step box while working and hold up a "?" when they need you. Turn on gravity and they drop onto your taskbar and wander around.

**回复 2**：

Works with Claude Code, Codex, Cursor, OpenCode and a few more over MCP. Just 3 tools, ~30-token receipts, and nothing leaves your machine. Windows only for now.

**封面**：`launch-covers/x.png`（16:9）。发纯图帖时用它。帖子里贴 GitHub 链接时，X 会显示仓库的 Social preview，把 `launch-covers/github-social.png` 设为仓库的 Social preview（仓库 Settings → Social preview）即可。

---

## YouTube

**缩略图**：`launch-covers/youtube.png`（1280×720）
**视频**：`agentkanban-promo-en-pets.mp4`

**标题**（二选一）：
- AgentKanban: a task board your coding agents keep themselves
- I made my coding agents keep their own task board

**描述**：

Where agents report and you sign off.

When you run a few coding agents at once, it gets hard to tell who is doing what and which task is waiting on you. AgentKanban is a small always-on-top window. Through a local MCP server, agents register their own tasks, write a plan and report progress at each milestone. You only get pinged when something needs your input, and finished work turns gray until you accept it.

It also comes with pixel desktop pets: one per agent, acting out its status on top of the board. Drag them anywhere, click one to open its task, or turn on gravity mode and watch them wander along your taskbar.

- Works with Claude Code, Codex, Cursor, OpenCode, Antigravity, DeepSeek Harness and Hermes
- 3 MCP tools, ~30-token write receipts
- Runs fully on your machine: no model API calls, no uploads, no account
- Windows, MIT licensed

Download and source: https://github.com/keviccz/AgentKanban

**标签**：coding agents, Claude Code, Codex, Cursor, MCP, AI coding, developer tools, kanban, desktop pet, open source

> 要发 Shorts，需要一版英文竖版视频（现在只有中文竖版），可以用同一套代码出一版。

---

## B 站

**封面**：`launch-covers/bilibili.png`（16:9，1920×1080）和 `launch-covers/bilibili-4x3.png`（4:3，1440×1080）。上传时 B 站会分别要横版和 4:3 封面，4:3 那张用于手机端的双列推荐流。
**视频**：`agentkanban-promo-pets.mp4`（中文横版，34 秒，带桌宠）
**分区**：科技 → 软件应用（或 计算机技术）

**标题**：让 Coding Agent 自己记账：一个会自己更新的任务浮窗（还有像素桌宠）

**简介**：

同时跑几个 Coding Agent 的时候，最累的不是写代码，是记住谁在做什么、哪件事在等我回话。

AgentKanban 是一个常驻桌面的小浮窗。Agent 通过本地 MCP 自己登记任务、写计划、在阶段完成时更新进度；只有受阻或需要你补充时才会弹通知；做完的任务直接变灰，右键一键验收。每个 Agent 还有一个像素桌宠，站在看板上把状态演出来，开重力模式还会掉到任务栏上溜达。

- 支持 Claude Code、Codex、Cursor、OpenCode 等 7 个客户端，一键接入
- 只有 3 个 MCP 工具，写入回执约 30 token，不怎么占上下文
- 完全本地运行：不调模型 API，不上传，不用注册
- 目前支持 Windows，MIT 开源

下载和源码：https://github.com/keviccz/AgentKanban

**标签**：AI编程、ClaudeCode、Codex、Cursor、MCP、开源软件、效率工具、桌宠、程序员

> 置顶评论：GitHub 链接，再加一句「有问题或想要的功能直接评论区说」。B 站观众爱在评论区提需求，积极回复能带动推荐。

---

## 还值得发的地方

| 渠道 | 为什么值得发 | 怎么发 |
|---|---|---|
| V2EX「分享创造」 | 程序员集中，对开源小工具很友好 | 用下面的 V2EX 文案，回复要跟上 |
| HelloGitHub | 月刊专门收录有趣的开源项目，带来长期稳定的 star | 在 hellogithub.com 提交项目 |
| 阮一峰《科技爱好者周刊》 | 读者多，收录开源工具 | 在 GitHub 的 ruanyf/weekly 仓库开 issue 自荐 |
| 即刻 | AI 和独立开发者圈子活跃 | 发桌宠动图，配一句话，再附链接 |
| 少数派 | 适合「我的 AI 编程工作流」这类长文 | 把知乎文章改写后投稿到 Matrix |
| 掘金 / Linux.do | 技术读者多 | 发知乎文章，Linux.do 发在「开发调优」类版块 |
| Hacker News（Show HN） | 独立开发工具的最大曝光渠道 | 用下面的 Show HN 文案，美东工作日上午发，自己先写首条评论 |
| Reddit | r/ClaudeAI、r/ChatGPTCoding、r/cursor、r/mcp、r/SideProject 都是目标用户 | 用下面的 Reddit 文案；先看各版规则，有的版每周只允许一次自我推广 |
| Product Hunt | 集中一天的曝光，也方便做背书 | 准备好图集（封面、截图、GIF），周二到周四发 |
| MCP 目录 | 有人专门搜 MCP，会带来长期搜索流量 | 提交到 mcp.so、Smithery、Glama、PulseMCP，给 awesome-mcp-servers 提 PR |
| GitHub 仓库本身 | 所有分享链接都会显示仓库卡片 | 设置 Social preview（用 `github-social.png`），加上 topics：mcp、claude-code、codex、cursor、kanban、tauri、desktop-pet |

### V2EX「分享创造」

**标题**：做了个让 Coding Agent 自己记账的桌面看板，开源，还带像素桌宠

**正文**：

同时开 Claude Code 和 Codex 以后，我经常记不清哪个在做什么、哪件事在等我回话，于是写了 AgentKanban。

它是一个常驻桌面的小浮窗，带本地 MCP。Agent 做会改文件的任务时，自己登记目标、计划和步骤，只在阶段完成、受阻或做完时更新。只有需要你补充时才弹通知，完成的任务变灰，等你验收。

- 3 个 MCP 工具，写入回执约 30 token
- SQLite + WAL，多个 Agent 同时写也没问题
- 全部本地运行，不调模型 API，不用账号
- 每个 Agent 一个像素桌宠，可拖动，可以开重力模式让它们在任务栏上溜达

Tauri + Rust，目前只支持 Windows，MIT。

https://github.com/keviccz/AgentKanban

欢迎拍砖，也想知道大家多开 Agent 时怎么管任务。

### Show HN

**Title**：Show HN: AgentKanban – a desktop board your coding agents keep themselves

**Text**：

I run Claude Code and Codex side by side and kept losing track of which one was doing what, and which task was quietly waiting on me.

AgentKanban is a small always-on-top window (Tauri + Rust) with a local stdio MCP server. When an agent starts a task that changes files, it registers a goal, acceptance checks and plan steps, then updates the task only at milestones, when blocked, or when done. You get a notification only when something needs your input; finished tasks turn gray until you accept them.

A few choices I cared about: only 3 MCP tools, and writes return ~30-token receipts so tracking doesn't eat the agent's context. SQLite with WAL so several agents can write at once. Everything stays local: no model calls, no account.

It also has optional pixel desktop pets that act out each agent's status. I added them for fun, but glancing at them turned out to be the quickest way to see who's waiting on me.

Windows only for now, MIT licensed. I'd love feedback, especially from people who run several agents at once.

https://github.com/keviccz/AgentKanban

### Reddit

**Title**：I built a desktop board that my coding agents update themselves (open source, with pixel desktop pets)

**Text**：

Running Claude Code and Codex at the same time, I kept forgetting which agent was on what and which task was waiting for my input. So I made AgentKanban.

Agents register their own tasks over a local MCP server (goal, plan steps, progress) and update them only at milestones. You get pinged only when one is blocked or needs you; done tasks turn gray until you sign off. Each agent also gets a little pixel pet on top of the board that shows what it's doing, and with gravity mode on they wander along your taskbar.

3 MCP tools, tiny receipts, fully local, no account. Windows only for now, MIT.

GitHub: https://github.com/keviccz/AgentKanban

Happy to answer questions or take feature requests.
