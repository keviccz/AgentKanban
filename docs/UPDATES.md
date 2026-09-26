# 更新与发布

## 使用更新功能

在设置中查看当前版本、手动检查更新和阅读版本说明。默认开启自动检查，后台检查最多每天一次；可选择发现更新时自动在后台下载，也可以手动下载。下载期间仍可使用看板，安装需要点击安装按钮。

下载完成并通过签名校验后，更新才进入可安装状态。安装前检查当前程序目录的 MCP 是否仍在使用：有占用时在本次应用运行期间保留下载结果，等待客户端结束对应连接后重试安装；不会结束正在工作的 Agent。关闭设置或隐藏到托盘不丢失下载，完全退出应用后需要重新下载。通过检查后先备份 SQLite 到数据目录的 `backups/`，备份失败则停止安装。安装程序随后接管并退出旧浮窗。

检查或下载失败可以重试，当前程序和数据继续可用。仓库还没有正式 Release 时会显示尚未发布；网络、证书、服务或签名错误会单独显示。旧版 0.4 没有应用内更新入口，首次进入 0.5 需使用安装包；之后的新版本才可走应用内更新。

更新只访问版本信息和安装包地址，不上传任务内容，也不调用 Agent 或增加 MCP 工具。更新功能不负责重新连接客户端中的 MCP，更新后客户端应重新加载新程序。

## 构建产物

所有构建共用 `scripts/build.ps1`：先构建 MCP 侧车，再构建桌面与安装包，最后整理安装包和便携 ZIP。没有签名密钥时仍可生成供本地安装的普通产物，但不会生成可供应用内更新使用的清单。

签名构建需要 `TAURI_SIGNING_PRIVATE_KEY`（密钥文件绝对路径或完整内容），加密密钥另需 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。配置中的 `plugins.updater.pubkey` 必须是对应公钥。私钥留在受保护的本机目录或 CI Secret，不能提交进 Git。本机当前使用的密钥存放于 `%USERPROFILE%\.codex\signing\agentkanban\updater.key`；保管和备份此文件，后续版本需保持同一签名身份。

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = '<签名密钥文件的绝对路径>'
pwsh -NoProfile -File .\scripts\build.ps1 -RequireSignature
```

可更新产物包含原 NSIS 安装包、它的 `.sig` 签名，以及 `latest.json`。清单中的版本、平台、下载文件名和签名必须与该次构建对应，不复用旧构建残留。Windows 平台标识为 `windows-x86_64`，应用从 `https://github.com/keviccz/AgentKanban/releases/latest/download/latest.json` 检查正式最新版本。

构建和独立清单生成都会使用 Node 内置密码学，对实际安装包和可信注释执行完整签名验证，并核对应用内公钥及签名中的版本。错误公钥、篡改内容或签名会阻止生成更新清单，不能仅以打包工具退出成功判断签名有效。

## GitHub 发布流程

仓库提供 Windows 发布工作流：推送 `v*` 标签，或手动选择一个已存在的标签后运行。工作流准备 MCP 后执行 Rust 测试、Clippy、格式与发布脚本检查，再核对标签与 npm、Rust 和 Tauri 的版本一致，完成前端与签名构建，然后创建或更新该版本的 **草稿 Release**。发布后的 Release 不会被工作流覆盖。

应用内版本说明和 GitHub 草稿说明均来自 `docs/RELEASE_NOTES.md`。本机构建可用 `-NotesFile <路径>` 指定其他说明文件；正式工作流使用仓库中的该文件，发布前应更新为对应版本内容。

首次使用工作流前，在仓库 Actions Secrets 配置 `TAURI_SIGNING_PRIVATE_KEY`，如有密码再配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。公钥随应用发布；私钥仅用于构建时签名。不要将密钥内容写入日志、命令参数、Issue 或 Release 说明。

检查草稿的版本说明和产物、完成安装验收后，再将它正式发布。草稿不供已安装客户端发现，推送源码也不会自动触发安装或正式发布。发布升级前需确认数据库迁移兼容范围；数据备份不是任意旧版本都能直接读取新数据库的保证。

实现采用 [Tauri 官方更新插件](https://v2.tauri.app/plugin/updater/) 的签名校验和静态更新清单；分发使用 [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)。
