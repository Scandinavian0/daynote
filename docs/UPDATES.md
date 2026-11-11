# 应用内更新与发布

从 0.1.1 起，桌面主窗口启动约 3 秒后检查 GitHub 最新正式 Release，保持运行时每 6 小时再查一次。应用只读取版本清单；用户选择“下载并安装”后才下载。选择“稍后”后，本次运行不会重复提醒同一版本；设置页手动检查不受此限制。托盘启动时提示留在主窗口，下次打开即可看到，不会强制抢焦点。

设置 → 应用更新中可以查看当前版本、检查结果和失败原因。断网或 GitHub 暂时不可用不影响备忘功能，可稍后手动重试。开发浏览器预览不会联网检查更新。

下载后先验证更新包签名和签名中绑定的版本。当前发布源固定为本仓库的 `releases/latest/download/latest.json`，不接受界面传入任意下载地址；不更新到相同、较旧或预发布版本。安装前等待所有日期的便签草稿保存，失败时保留草稿并暂停安装。安装阶段会关闭应用，NSIS 完成后重新启动。请先保存尚未提交的任务/速记输入，并退出占用 CLI/MCP 程序的外部客户端。

更新沿用注册的安装目录，保留任务和设置。更新签名与 Windows Authenticode 代码签名是两套机制，本项目尚未提供后者。0.1.0 没有检查更新代码，需要手动安装一次 0.1.1。

## 维护者发布流程

1. 同步 `package.json`、`package-lock.json`、工作区 `Cargo.toml` / `Cargo.lock` 和 `src-tauri/tauri.conf.json` 的版本。
2. 新增 `docs/releases/v版本号.md` 并更新变更记录。
3. 本地执行前端检查/测试和 `cargo test --workspace --locked`，审查变更。
4. 将审核后的提交推送至 `main`；Windows CI 检查通过后，再创建并推送同版本的 `v版本号` 标签。
5. 标签工作流重新验证构建、签名、旧版覆盖升级、快捷方式和卸载数据保留；全部通过后发布安装包、`.sig`、`SHA256SUMS.txt` 与 `latest.json`，最后设为最新正式版。

正式构建需要仓库 Secret `TAURI_SIGNING_PRIVATE_KEY`，对应的公钥嵌入 `tauri.conf.json`。私钥必须在仓库外安全备份，不能提交到 Git 或随安装包分发。该私钥丢失将影响已安装版本的后续更新，不要随意替换公钥。

本地正式构建可在进程环境中设置 `TAURI_SIGNING_PRIVATE_KEY`（文件路径或密钥内容）以及 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，然后运行 `npm run release:build`。脚本要求签名包含版本字段，并生成静态更新清单。运行下面的验证，使用真正的安装包和生产公钥检查下载与签名，不执行安装程序：

```powershell
cargo test -p daynote-app updates::tests::signed_release_artifact_is_accepted_by_the_production_key --locked -- --ignored
```

没有维护者私钥的贡献者可以运行 `npm run release:build -- --unsigned` 构建本地测试安装包。该模式不生成更新清单和签名，不能用作自动更新发布。Fork 必须配置自己的更新地址、密钥和仓库发布权限。

## 验证边界

前端使用实际 Store 与 Svelte DOM 回归检查确认、稍后、进度、重试及便签保存失败；原生更新命令使用本地 HTTP 测试源验证版本判断、签名错误、伪造版本和下载前置条件。Windows CI 验证生产签名安装包、从 0.1.0 的 `/UPDATE` 覆盖升级、程序启动、数据和桌面快捷方式。CI 不控制用户桌面来点击真实应用里的安装按钮；此按钮到安装器的衔接由前端流程、原生命令与 NSIS 升级分别覆盖。
