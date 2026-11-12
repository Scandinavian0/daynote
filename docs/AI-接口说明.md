# DayNote AI 接口说明

DayNote 是本机的每日备忘/待办工具。AI 代理（Codex、Claude Code 或任何脚本）可以把规划**写进去**，也可以**读出**哪些做完了、哪些没做完。以下接法读写的是同一个数据库，共用同一套业务工具。

| 接法 | 入口 | 应用没开时 |
|---|---|---|
| 共享 MCP（推荐，Streamable HTTP） | `http://127.0.0.1:<实际端口>/mcp` | 不可用（随应用运行） |
| 命令行 | `<安装目录>\daynote-cli.exe <命令>` | 可用 |
| REST | `http://127.0.0.1:<实际端口>/v1/...` | 不可用（随应用运行） |
| 兼容 MCP stdio | `<安装目录>\daynote-cli.exe mcp` | 可用 |

共享模式由唯一的 DayNote 桌面应用提供同一服务，多个聊天通过 URL 连接，不会为每个聊天启动常驻 CLI。应用运行（含托盘驻留）时可用，完全退出后停止。MCP 与 REST 共用本地 HTTP 开关、端口和 Bearer 访问令牌。设置页仅在服务实际启动后显示运行地址；默认端口是 `47821`，以设置页显示的地址为准。

安装升级保留已有数据，不自动迁移客户端配置。已有 stdio 用户需自行将原定义替换为 URL 配置；继续使用兼容 stdio 时，每个聊天仍会创建独立的 CLI 进程。

应用开着时，写入的内容约 1 秒内出现在面板上，带提醒时间的任务到点会弹卡片。

## 使用约定

- **日期和时间都是本机本地时间**。日期 `YYYY-MM-DD`；时间 `YYYY-MM-DD HH:MM`，或只写 `HH:MM`（表示任务所在那天的那个时刻）。省略 `date` 就是今天。
- 每个返回都带 `today` 和 `now`，判断"今天"以它们为准，不要自己推算。
- **写之前先读**：先 `list_tasks` 或 `get_summary` 看当天已有什么，避免重复添加。
- 一件事拆步骤用 `subtasks`（只有一层）。最后一个步骤完成时父任务自动完成。
- 删除是软删除，`restore_task` 可以找回。
- 每条任务记录是谁写的（`source`）和谁完成的（`completed_by`）。AI 写入的条目在面板上带 `AI` 标记。

## 工具

| 工具 | 用途 | 主要参数 |
|---|---|---|
| `list_tasks` | 查某天或某段日期的任务 | `date` 或 `from`+`to`；`status`: `all` / `todo` / `done` / `overdue` |
| `get_summary` | 完成情况统计 + 所有逾期未完成 | `date` 或 `from`+`to` |
| `add_tasks` | 批量写入某天的规划 | `date`，`tasks: [{title, note?, remind_at?, priority?, subtasks?}]` |
| `add_subtasks` | 给已有任务加步骤 | `parent_id`，`subtasks` |
| `update_task` | 改标题/备注/日期/提醒/优先级 | `id` + 要改的字段；`remind_at: null` 清除提醒 |
| `complete_task` | 标记完成或改回未完成 | `id`，`done`（默认 true） |
| `move_tasks` | 把一批任务挪到某天 | `ids`，`date` |
| `reorder_tasks` | 重排某天的顺序 | `date`，`ids` |
| `delete_task` / `restore_task` | 删除 / 撤销删除 | `id` |
| `get_day_note` / `set_day_note` | 读写当日便签（整段覆盖） | `date`，`content` |
| `add_recurring` | 新建重复任务 | `title`，`rule`，`remind_time?`，`start_date?`，`end_date?` |
| `list_recurring` / `update_recurring` / `delete_recurring` | 管理重复任务 | `id` + 字段，`active` |

`status: "overdue"` 返回所有日期早于今天且未完成的任务，忽略日期参数。
重复规则 `rule`：`daily`、`weekdays`、`weekly:1,3,5`（1=周一…7=周日）、`monthly:15`。
完整的参数说明和 JSON Schema：`daynote-cli.exe tools`。

## 典型用法

**把今天的规划写进去**（`add_tasks`）：

```json
{
  "tasks": [
    {
      "title": "准备项目演示",
      "remind_at": "15:00",
      "subtasks": ["整理提纲", "制作演示材料", "检查示例"]
    },
    { "title": "回复合作伙伴", "priority": 1, "note": "补充项目时间安排" }
  ]
}
```

**看今天做完了哪些、还剩哪些**：`list_tasks` 不带参数，按每条的 `status` 和 `completed_at` 判断；只要数字用 `get_summary`。

**看这一周**：`get_summary`，参数 `{"from": "2026-11-09", "to": "2026-11-15"}`。返回里的 `overdue` 是所有遗留事项。

**把遗留的事挪到今天**：先 `list_tasks {"status": "overdue"}` 拿到 id，再 `move_tasks {"ids": [...], "date": "<today>"}`。

## 命令行

```
daynote-cli list [--date D] [--from D --to D] [--status all|todo|done|overdue]
daynote-cli add "标题" [--date D] [--remind T] [--note N] [--important]
daynote-cli done <id> | undone <id> | delete <id> | restore <id>
daynote-cli summary [--date D | --from D --to D]
daynote-cli call <工具名> [<JSON 参数> | @文件 | -]
daynote-cli tools
```

- 输出是 UTF-8 的 JSON；出错时输出 `{"error": {"code", "message"}}` 且退出码非 0。
- 加 `--source <名字>` 标明是谁在写，例如 `--source codex`。
- `call` 可以调上表任何工具；参数较长时写进文件用 `@文件`，或用 `-` 从标准输入读。

## 本地 REST

- 仅本机可访问。除 `/v1/health` 外都要带请求头 `Authorization: Bearer <令牌>`。令牌在设置页显示的数据目录中的 `api-token.txt`，新安装默认 `%LOCALAPPDATA%\DayNoteData\api-token.txt`。
- `GET /v1/health`、`GET /v1/tools`、`GET /v1/tasks?date=&from=&to=&status=`、`GET /v1/summary?date=&from=&to=`、`POST /v1/call/<工具名>`（请求体是 JSON 参数）。
- 可选请求头 `X-DayNote-Source: <名字>` 标明来源。
- 带 `Origin` 头的请求（来自网页）一律被拒绝。
- 端口和开关在应用的设置页里；端口允许 `1024–65535`。保存后使用界面显示的运行地址，端口占用时会显示失败并保留原服务。关闭接口时可以保存下次启动使用的端口。

这里配置的是 AI 客户端访问 DayNote 的本地接口，不是模型服务的 API Key。共享 MCP 同样使用 `Authorization: Bearer <令牌>`；只有兼容 stdio 不使用 HTTP 令牌。

## 接入共享 MCP（推荐）

先启动 DayNote，在设置 → AI 接口确认“正在监听”，复制共享 MCP 地址，并从页面显示的令牌文件中读取内容。下面的 URL 使用默认端口，请按实际运行地址修改；`<DAYNOTE_TOKEN>` 仅为占位符，必须替换为文件中的令牌，不能填文件路径。`/mcp` 是 MCP 入口，`/v1` 是 REST 入口，两者不能互换。所有配置只适用于运行 DayNote 的同一台电脑。

Codex（当前 `CODEX_HOME` 下的 `config.toml`）：

```toml
[mcp_servers.daynote]
url = "http://127.0.0.1:47821/mcp"
http_headers = { Authorization = "Bearer <DAYNOTE_TOKEN>", "x-daynote-source" = "codex" }
```

把现有 `[mcp_servers.daynote]` 中的 `command`、`args` 和 stdio 专用环境配置替换为上述 HTTP 配置，不要追加第二份同名定义。配置字段参考 [Codex 官方 MCP 文档](https://learn.chatgpt.com/docs/extend/mcp#streamable-http-servers)。

Claude Code（MCP JSON 配置中的 `mcpServers` 条目）：

```json
{
  "mcpServers": {
    "daynote": {
      "type": "http",
      "url": "http://127.0.0.1:47821/mcp",
      "headers": {
        "Authorization": "Bearer <DAYNOTE_TOKEN>",
        "x-daynote-source": "claude"
      }
    }
  }
}
```

例如项目级 `.mcp.json` 使用上面的完整结构；用户级 `~/.claude.json` 保留其他内容，只替换已有的 `mcpServers.daynote` 条目。HTTP 配置必须带 `type: "http"`。配置字段参考 [Claude Code 官方 MCP 文档](https://code.claude.com/docs/en/mcp#option-1-add-a-remote-http-server)。

保存后重新连接或重启客户端，再调用 `get_summary` 验证连接。端口变化后同步更新 URL。重新生成令牌后，旧配置立即失效，需要重新读取令牌文件、更新两个客户端中的 Bearer 值并重新连接；DayNote 不会自动改写客户端配置。静态令牌属于本机凭据，不要把实际值提交到仓库或分享给他人。

## 兼容 MCP stdio

仅在需要保留已有接法或不运行桌面应用时使用。设置页的“旧版 MCP stdio 兼容方式”提供实际安装位置生成的命令；它会为每个连接创建一个独立的常驻 CLI。

Claude Code：

```text
claude mcp add --scope user daynote -- "C:\path\to\DayNote\daynote-cli.exe" mcp
```

Codex：

```toml
[mcp_servers.daynote]
command = 'C:\path\to\DayNote\daynote-cli.exe'
args = ["mcp"]
```

上面的 CLI 路径是占位示例。在设置页更改数据目录时，重启 DayNote 之前所有接法继续使用原目录；重启后共享 MCP 使用新目录，客户端需重新连接并更新为新目录中的令牌。已在运行的兼容 stdio 服务在下一次调用时会自动跟上。若使用环境变量 `DAYNOTE_DATA_DIR`，GUI 与兼容 stdio 客户端需设置同一个值，修改后两者都要重启。切换目录不会自动搬迁或合并数据，详见 [安装与数据说明](INSTALL.md)。
