# DayNote AI 接口说明

DayNote 是本机的每日备忘/待办工具。AI 代理（Codex、Claude Code 或任何脚本）可以把规划**写进去**，也可以**读出**哪些做完了、哪些没做完。三种接法读写的是同一个数据库，行为完全一致。

| 接法 | 入口 | 应用没开时 |
|---|---|---|
| MCP（推荐） | `<安装目录>\daynote-cli.exe mcp` | 可用 |
| 命令行 | `<安装目录>\daynote-cli.exe <命令>` | 可用 |
| HTTP | `http://127.0.0.1:47821/v1/...` | 不可用（随应用运行） |

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

## HTTP

- 仅本机可访问。除 `/v1/health` 外都要带请求头 `Authorization: Bearer <令牌>`。令牌在设置页显示的数据目录中的 `api-token.txt`，新安装默认 `%LOCALAPPDATA%\DayNoteData\api-token.txt`。
- `GET /v1/health`、`GET /v1/tools`、`GET /v1/tasks?date=&from=&to=&status=`、`GET /v1/summary?date=&from=&to=`、`POST /v1/call/<工具名>`（请求体是 JSON 参数）。
- 可选请求头 `X-DayNote-Source: <名字>` 标明来源。
- 带 `Origin` 头的请求（来自网页）一律被拒绝。
- 端口和开关在应用的设置页里；端口允许 `1024–65535`。保存后使用界面显示的运行地址，端口占用时会显示失败并保留原服务。关闭接口时可以保存下次启动使用的端口。

这里配置的是 AI 客户端访问 DayNote 的本地接口，不是模型服务的 API Key。MCP 通过 stdio 通信，不使用 HTTP 令牌。

## 接入方式

下面的路径是占位示例，请替换为设置页显示的 CLI 路径。如果使用自定义数据目录，为 GUI 和 MCP 客户端设置同一个 `DAYNOTE_DATA_DIR`。

Claude Code：

```
claude mcp add --scope user daynote -- "C:\path\to\DayNote\daynote-cli.exe" mcp
```

Codex（`config.toml`）：

```toml
[mcp_servers.daynote]
command = 'C:\path\to\DayNote\daynote-cli.exe'
args = ["mcp"]
```
