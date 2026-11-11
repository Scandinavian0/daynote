//! Tool descriptions and JSON Schemas, written for AI clients.
use crate::api::ToolSpec;
use serde_json::{json, Value};

const DATE: &str = "日期，格式 YYYY-MM-DD（本机本地日期）。省略表示今天。";
const DT: &str = "提醒时间，本机本地时间，无时区。可写 \"YYYY-MM-DD HH:MM\"、\"YYYY-MM-DDTHH:MM[:SS]\"，或只写 \"HH:MM\"（表示任务所在日期的那个时刻）。";

fn date() -> Value {
    json!({"type": "string", "description": DATE, "pattern": "^\\d{4}-\\d{2}-\\d{2}$"})
}

fn id(desc: &str) -> Value {
    json!({"type": "integer", "description": desc})
}

fn range_props() -> Value {
    json!({
        "date": date(),
        "from": {"type": "string", "description": "范围起始日期 YYYY-MM-DD（含）。与 to 同时给出，不能与 date 同用；范围最长 366 天。"},
        "to": {"type": "string", "description": "范围结束日期 YYYY-MM-DD（含）。与 from 同时给出。"}
    })
}

fn subtasks_schema() -> Value {
    json!({
        "type": "array",
        "description": "子任务列表（只有一层）。每项可以是字符串标题，或 {title, note?}。",
        "items": {
            "anyOf": [
                {"type": "string"},
                {"type": "object", "properties": {"title": {"type": "string"}, "note": {"type": "string"}}, "required": ["title"]}
            ]
        }
    })
}

fn priority() -> Value {
    json!({"type": "integer", "enum": [0, 1], "description": "优先级：0 普通，1 重要。"})
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn rule() -> Value {
    json!({"type": "string", "description": "重复规则：daily（每天）、weekdays（周一到周五）、weekly:1,3,5（每周这几天，ISO 星期 1=周一…7=周日）、monthly:15（每月 15 号；当月没有这一天就取当月最后一天）。"})
}

pub(crate) fn all() -> Vec<ToolSpec> {
    let mut v = vec![
        ToolSpec {
            name: "list_tasks",
            description: "列出任务。省略日期参数表示今天；可用 date 查某一天，或用 from+to 查一段日期。status：all（默认，全部）、todo（未完成）、done（已完成）、overdue（截至今天所有逾期未完成的任务，即日期早于今天且未完成；此时忽略日期参数）。返回顶层任务按日期和顺序排列，子任务嵌套在 subtasks 里。返回里的 today/now 是本机当前日期和时间，请以它为准判断“今天”。",
            input_schema: {
                let mut p = range_props();
                p["status"] = json!({"type": "string", "enum": ["all", "todo", "done", "overdue"], "description": "all=全部（默认）；todo=未完成；done=已完成；overdue=所有日期早于今天且未完成的任务（忽略日期范围）。"});
                obj(p, &[])
            },
            public: true,
        },
        ToolSpec {
            name: "add_tasks",
            description: "批量给某一天添加任务（追加到当天末尾），可同时带子任务。date 省略表示今天。返回新建的任务（含 id）。写入前建议先 list_tasks 看看当天已有什么，避免重复。",
            input_schema: obj(
                json!({
                    "date": date(),
                    "tasks": {
                        "type": "array",
                        "minItems": 1,
                        "description": "要添加的任务。",
                        "items": obj(json!({
                            "title": {"type": "string", "description": "任务标题，不能为空。"},
                            "note": {"type": "string", "description": "备注，可选。"},
                            "remind_at": {"type": "string", "description": DT},
                            "priority": priority(),
                            "subtasks": subtasks_schema()
                        }), &["title"])
                    }
                }),
                &["tasks"],
            ),
            public: true,
        },
        ToolSpec {
            name: "add_subtasks",
            description: "给已有的顶层任务追加子任务（子任务只有一层，日期跟随父任务）。若父任务已完成，加入未完成的子任务后父任务会回到未完成。返回父任务（含全部子任务）。",
            input_schema: obj(json!({"parent_id": id("父任务 id（必须是顶层任务）。"), "subtasks": subtasks_schema()}), &["parent_id", "subtasks"]),
            public: true,
        },
        ToolSpec {
            name: "update_task",
            description: "修改任务：只传要改的字段。改 date 会把任务挪到那一天（子任务跟着走，提醒平移到新日期的同一时刻，“挪到明天”就用它或 move_tasks）；子任务不能单独改日期。remind_at 传 null 清除提醒，note 传 null 清除备注。提醒时间一旦变化，之前的“已提醒/稍后提醒”状态会重置。",
            input_schema: obj(
                json!({
                    "id": id("任务 id。"),
                    "title": {"type": "string"},
                    "note": {"type": ["string", "null"]},
                    "date": {"type": "string", "description": "新日期 YYYY-MM-DD（仅顶层任务）。"},
                    "remind_at": {"type": ["string", "null"], "description": format!("{DT} 传 null 清除提醒。")},
                    "priority": priority()
                }),
                &["id"],
            ),
            public: true,
        },
        ToolSpec {
            name: "complete_task",
            description: "标记任务完成（done 默认 true）或改回未完成（done=false）。联动规则：完成父任务会同时完成它所有未完成的子任务；最后一个未完成的子任务完成后父任务自动完成；把已完成父任务下的子任务改回未完成，父任务也回到未完成；取消父任务完成不会改动子任务。",
            input_schema: obj(json!({"id": id("任务 id。"), "done": {"type": "boolean", "description": "true=完成（默认），false=改回未完成。"}}), &["id"]),
            public: true,
        },
        ToolSpec {
            name: "move_tasks",
            description: "把一批顶层任务挪到某一天（例如把逾期任务“挪到今天”或“挪到明天”），追加到那天末尾；子任务跟着走，提醒平移到新日期的同一时刻。重复任务的实例被挪走后会脱离所属系列。",
            input_schema: obj(json!({"ids": {"type": "array", "items": {"type": "integer"}, "minItems": 1, "description": "顶层任务 id 列表。"}, "date": {"type": "string", "description": "目标日期 YYYY-MM-DD。"}}), &["ids", "date"]),
            public: true,
        },
        ToolSpec {
            name: "reorder_tasks",
            description: "按给定顺序重排某一天的顶层任务。ids 里列出的排在前面，没列出的保持原有先后排在后面。返回重排后的当天任务。",
            input_schema: obj(json!({"date": {"type": "string", "description": "日期 YYYY-MM-DD。"}, "ids": {"type": "array", "items": {"type": "integer"}, "description": "当天顶层任务 id，按想要的顺序。"}}), &["date", "ids"]),
            public: true,
        },
        ToolSpec {
            name: "delete_task",
            description: "删除任务（软删除，可用 restore_task 恢复）。删除父任务会连带删除其子任务。已删除的任务不再出现在列表、统计和提醒里。",
            input_schema: obj(json!({"id": id("任务 id。")}), &["id"]),
            public: true,
        },
        ToolSpec {
            name: "restore_task",
            description: "撤销删除，恢复任务（以及当时随它一起被删的子任务）。",
            input_schema: obj(json!({"id": id("已删除任务的 id。")}), &["id"]),
            public: true,
        },
        ToolSpec {
            name: "get_summary",
            description: "统计某天或某段日期的完成情况（省略日期参数表示今天）：total/done/todo 和每天的明细 days，只统计顶层任务。overdue 与所查范围无关，是截至今天所有逾期未完成的任务。适合回答“今天/这周做完了哪些、还剩哪些”。",
            input_schema: obj(range_props(), &[]),
            public: true,
        },
        ToolSpec {
            name: "get_day_note",
            description: "读取某天的自由文本便签（每天一段）。date 省略表示今天。没有便签时 content 为空字符串。",
            input_schema: obj(json!({"date": date()}), &[]),
            public: true,
        },
        ToolSpec {
            name: "set_day_note",
            description: "写入（整段覆盖）某天的自由文本便签。date 省略表示今天。若想追加，请先 get_day_note 读出原文再拼接。",
            input_schema: obj(json!({"date": date(), "content": {"type": "string", "description": "便签全文。"}}), &["content"]),
            public: true,
        },
        ToolSpec {
            name: "add_recurring",
            description: "新建重复任务系列。实例按需生成：只从今天（或 start_date，取较晚者）起生成，绝不补生成今天之前的实例，最远生成到今天之后 366 天。",
            input_schema: obj(
                json!({
                    "title": {"type": "string"},
                    "note": {"type": "string"},
                    "rule": rule(),
                    "remind_time": {"type": "string", "description": "每个实例的提醒时刻 HH:MM（本机本地时间），可选。"},
                    "start_date": {"type": "string", "description": "开始日期 YYYY-MM-DD，省略表示今天。"},
                    "end_date": {"type": "string", "description": "结束日期 YYYY-MM-DD（含），可选。"}
                }),
                &["title", "rule"],
            ),
            public: true,
        },
        ToolSpec {
            name: "list_recurring",
            description: "列出所有重复任务系列（含已停用的，active=false）。",
            input_schema: obj(json!({}), &[]),
            public: true,
        },
        ToolSpec {
            name: "update_recurring",
            description: "修改重复任务系列，只传要改的字段；active=false 停用、true 重新启用。修改或停用后，明天及以后仍未完成的实例会按新设置重建；今天及以前的实例不变。",
            input_schema: obj(
                json!({
                    "id": id("系列 id。"),
                    "title": {"type": "string"},
                    "note": {"type": ["string", "null"]},
                    "rule": rule(),
                    "remind_time": {"type": ["string", "null"], "description": "HH:MM，传 null 取消提醒。"},
                    "start_date": {"type": "string", "description": "YYYY-MM-DD"},
                    "end_date": {"type": ["string", "null"], "description": "YYYY-MM-DD，传 null 取消结束日期。"},
                    "active": {"type": "boolean"}
                }),
                &["id"],
            ),
            public: true,
        },
        ToolSpec {
            name: "delete_recurring",
            description: "删除重复任务系列：明天及以后仍未完成的实例一并删除，今天及以前的实例保留。",
            input_schema: obj(json!({"id": id("系列 id。")}), &["id"]),
            public: true,
        },
    ];
    v.extend([
        ToolSpec {
            name: "due_reminders",
            description: "（内部）返回此刻应弹出的提醒：未完成、有提醒、生效时刻（稍后提醒时间或提醒时间）已到、且在该时刻之后还没提醒过的任务。会先生成今天的重复任务实例。",
            input_schema: obj(json!({}), &[]),
            public: false,
        },
        ToolSpec {
            name: "mark_reminded",
            description: "（内部）记录任务已提醒（reminded_at = 现在）。",
            input_schema: obj(json!({"id": id("任务 id。")}), &["id"]),
            public: false,
        },
        ToolSpec {
            name: "snooze_task",
            description: "（内部）稍后提醒：until 为具体时间（HH:MM 表示今天），或 minutes 分钟后。",
            input_schema: obj(json!({"id": id("任务 id。"), "until": {"type": "string"}, "minutes": {"type": "integer", "minimum": 1}}), &["id"]),
            public: false,
        },
        ToolSpec {
            name: "get_settings",
            description: "（内部）读取全部设置。",
            input_schema: obj(json!({}), &[]),
            public: false,
        },
        ToolSpec {
            name: "set_setting",
            description: "（内部）写入一项设置，值一律存为字符串。",
            input_schema: obj(json!({"key": {"type": "string"}, "value": {"type": "string"}}), &["key", "value"]),
            public: false,
        },
    ]);
    v
}
