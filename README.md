# DayNote

一个保存在本机的 Windows 每日备忘与待办工具。用桌面面板记录事项，也可让 Codex、Claude Code 和其他 MCP 客户端读写同一份数据。

## 下载安装

前往 [Releases](https://github.com/Scandinavian0/daynote/releases/latest)，下载 `DayNote-0.1.4-windows-x64-setup.exe` 并运行。

- 支持 Windows 10 / 11 x64；使用 Microsoft Edge WebView2 Runtime，缺少时安装程序会尝试联网安装。
- 默认安装到当前用户目录，无需管理员权限。安装后自动创建 **DayNote 桌面快捷方式**，同时提供开始菜单入口和卸载程序。
- 安装目录同时包含 `DayNote.exe` 和 `daynote-cli.exe`，不用另装 Python、Node.js 或 Rust。
- 本版本没有代码签名证书，Windows 可能显示“未知发布者”。可先将文件 SHA-256 与同一 Release 的 `SHA256SUMS.txt` 核对。

## 功能

- 每日任务、重要标记、单层子任务、排序、完成、软删除与恢复。
- 按日期浏览、逾期事项、每日便签和重复任务。
- 任务提醒、稍后提醒、晨间简报、晚间回顾。
- 系统托盘、可置顶面板、全局速记快捷键及开机启动设置。
- 共享 MCP（Streamable HTTP）、JSON 命令行和仅限本机的 REST 接口；保留旧版 MCP stdio 兼容方式。
- 自动检查新版本，提示确认后下载、验证签名并安装；设置页可手动检查。

设置的 **AI 接口** 主推共享 MCP 地址：`http://127.0.0.1:<实际端口>/mcp`。多个聊天连接同一个 DayNote 服务，无需为每个聊天启动常驻 CLI。DayNote 运行（含托盘驻留）时服务可用，完全退出后停止；已有 stdio 配置保留兼容，升级不会自动迁移客户端。

MCP 与 REST 共用设置里的 HTTP 开关、端口和 Bearer 令牌。端口有效范围为 `1024–65535`，只绑定 `127.0.0.1`；端口冲突会显示错误，保留原来的设置与服务。请使用界面显示的实际运行地址，以及令牌文件中的内容配置客户端。重新生成令牌后，需要重新读取文件、更新客户端配置并重新连接。设置页还可保存下次启动使用的数据目录。

“AI 接口”是外部 AI 代理访问备忘录的接口，不会调用模型服务，也不需要在 DayNote 中输入模型 API Key。详见 [AI 接口说明](docs/AI-接口说明.md)。

## 数据与隐私

新安装默认使用 `%LOCALAPPDATA%\DayNoteData`，包含 `daynote.db` 和 `api-token.txt`，与默认程序安装目录分开。数据仅写在本机；DayNote 没有账号、云同步或遥测。连接的 AI 客户端可能将读取的数据用于其模型请求，请按客户端自己的配置使用。

桌面应用会连接 GitHub 检查版本，确认更新后下载安装包；这些请求不包含备忘内容或本地访问令牌。更新行为及发布流程见 [应用内更新说明](docs/UPDATES.md)。

`DAYNOTE_DATA_DIR` 环境变量具有最高优先级，其次是设置页保存的数据目录。没有指定目录时，为兼容旧版，若程序旁已有 `data\daynote.db`，会继续使用该数据库；需要便携使用时，可在程序旁创建空文件 `portable.txt`。GUI 与 CLI 必须使用同一目录设置。

备份、更换目录和旧版迁移见 [安装与数据说明](docs/INSTALL.md)。卸载保留用户数据，避免意外丢失任务。

## 本地开发

需要 Windows x64、Rust MSVC 工具链（最低 Rust 1.85）、Visual Studio C++ Build Tools、WebView2，以及 Node.js 24 LTS。

```powershell
npm ci
npm run check
npm test
cargo test --workspace --locked
npm run tauri dev
```

仅预览 UI 可运行 `npm run dev`，此模式使用内存演示数据，刷新后重置。实际数据操作由 Tauri / CLI 执行。

构建安装包：

```powershell
npm run release:build -- --unsigned
```

输出在 `dist/`。构建脚本先编译 CLI，再将其与桌面程序、项目许可证和依赖许可说明一起打包，并生成 SHA-256 校验文件。首次构建需要网络下载 Rust/npm 依赖及 NSIS 工具。正式发布不加 `--unsigned`，需要更新签名私钥，详见更新说明。

## 代码结构

| 路径 | 内容 |
| --- | --- |
| `crates/core` | SQLite 存储、任务、重复事项及统一业务 API |
| `crates/cli` | JSON 命令行和兼容 MCP stdio 服务 |
| `src-tauri` | Windows 桌面程序、托盘、提醒、快捷键、共享 MCP / REST 服务与安装配置 |
| `ui` | Svelte 5 界面 |
| `scripts` | 发布构建脚本 |
| `docs` | 安装、AI 接口与发布记录 |

贡献请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)，安全问题见 [SECURITY.md](SECURITY.md)。项目采用 [MIT License](LICENSE)。
