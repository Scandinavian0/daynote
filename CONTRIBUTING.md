# 参与开发

欢迎通过 GitHub Issues 描述可复现问题，或提交小范围 Pull Request。

## 验证

```powershell
npm ci
npm run check
npm test
npm run build
cargo test --workspace --locked
```

修改安装逻辑时还应运行 `npm run release:build -- --unsigned`，在隔离的 Windows 用户或虚拟机中验证安装、桌面快捷方式、CLI/MCP 和卸载保留数据。正式更新包需要维护者签名，见 [更新与发布说明](docs/UPDATES.md)。

共享 MCP 的并发验收还需对运行中的应用执行 `node scripts/verify-sdk-mcp.mjs http://127.0.0.1:47821/mcp "<设置页中的令牌文件路径>" <实际版本>`。该脚本只读，默认运行 10 轮，每轮 20 个官方 SDK 客户端，包含后台 GET 探测及关闭后重连；不输出令牌或任务内容。安装器 CI 会自动执行。故障机制及确定性 TCP 回归见 [并发连接说明](docs/MCP-CONCURRENCY.md)。

- 在真实业务边界添加回归测试，先观察失败再修复。
- 保持 GUI、CLI、MCP 与 HTTP 共用 `daynote-core` 的业务规则。
- HTTP 仅绑定回环地址；保留令牌认证、Origin/Host 检查和请求大小限制。
- 不提交数据库、令牌、个人配置、应用运行目录或编译产物。
- 不在文档中写入个人电脑的绝对路径；使用可替换的示例。
- 修改公开接口时同步更新 `docs/AI-接口说明.md` 与变更日志。

此项目当前只发布 Windows x64 桌面安装包。其他平台的编译或功能未经验证。
