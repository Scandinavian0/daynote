# 0.1.0 发布验证

本次检查覆盖桌面设置、共享数据目录、HTTP、CLI/MCP、SQLite 业务逻辑、便签保存以及 Windows 打包。

## 已修复的审查问题

| 问题 | 验证方式 |
| --- | --- |
| 接口只读，无法修改端口 | 真实 Svelte 渲染回归；浏览器输入非法值与有效值并检查反馈 |
| 绑定失败仍保存新端口、丢失原服务 | 实际 socket 占用端口及数据库拒绝写入回归 |
| 不完整 HTTP 请求阻塞服务，巨大 Content-Length 带来内存风险 | 真实 socket 的部分请求、巨大声明长度和取消测试 |
| 令牌更新与持久化状态不一致的风险 | 文件替换失败、成功轮换后旧令牌 401 / 新令牌 200 |
| 安装版在受保护路径写数据库失败 | 复制实际 CLI 到隔离安装路径，检查默认数据目录和旧版兼容 |
| 稍后提醒分钟值溢出 | 公开业务调用返回 invalid_argument，任务保持原样 |
| 同秒删除/恢复误恢复子任务 | 公开业务调用回归、v1→v2 迁移回归 |
| 跨午夜、保存失败、延迟刷新覆盖便签 | 实际 Store 操作及异步保存/刷新测试 |

## 可复现命令

```powershell
npm ci
npm test
npm run check
npm run build
cargo test --workspace --locked
npm run release:build
```

Windows 工作流还在一次性的 GitHub-hosted runner 上运行 `scripts/verify-installer.ps1`，检查安装文件、桌面快捷方式目标与图标、已安装 CLI/MCP、卸载及数据库保留。它拒绝在普通用户环境或存在 DayNote 安装/数据时运行。

## 检查边界

- 浏览器验证使用开发演示后端；真实后端由 Rust 数据库与网络测试覆盖。
- 不完整网络请求可以取消；正在执行的同步数据库操作仍需等待 SQLite 返回，不能承诺硬实时中止。
- 当前仅发布 Windows x64。未验证其他系统、CPU 架构、所有 WebView2 版本或无 WebView2 的完全离线安装。
- 发布包没有代码签名证书；提供 SHA-256 校验文件。
- npm 生产依赖审计未发现已知问题；完整依赖审计报告构建链 `deepmerge` 与其上游 Svelte 插件共 2 条低等级提示，npm 当前未提供修复。它们属于构建工具链，不接受应用用户的运行时输入。未声称完成所有 Rust 依赖的漏洞审计或外部安全审计。
- 开发测试期间可能出现 Vite HMR 端口日志；独立缓存的回归测试完成结果以测试进程退出状态为准。

最终代码和工作流结果以对应 GitHub 提交及 [Actions](https://github.com/Scandinavian0/daynote/actions) 中的记录为准。
