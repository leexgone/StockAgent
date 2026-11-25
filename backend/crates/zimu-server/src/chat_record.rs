//! Web 会话档案状态机（T3/ADR-0011）：`WebEvent` 事件流 → 待落库行。
//!
//! **纯状态机、零 IO**——recorder 任务在 `agent_bridge` 接线调用，单测直接喂帧序列。
//! 档案词汇（与 docs/02 展示口径同源）：
//! - **user 行不经这里**：WebFrontend 不回广播用户输入；落库在注入前由
//!   `AgentBridge::submit_user_text` 完成（"先落库再注入"——避免"模型记得、
//!   档案没有"的反向分叉）；
//! - `thinking_delta` **丢弃**：框架「thinking 不写回 history」+ docs/02
//!   「不入会话档案、不占额度」；
//! - `tool_call`/`tool_result` 配对合成一条 **tool 行**（存档仅供展示与审计；
//!   **不回灌上下文**——回灌会把行情/持仓陈旧数字喂回模型，违反 03「派生数字现算」）；
//! - `turn_complete` 定稿 assistant 段；`/clear` 的固定回包
//!   `"[Conversation history cleared]"` 特判丢弃（档案里不留确认噪音）；
//! - recorder 自身丢帧（broadcast Lagged）→ 当轮 assistant 尾部如实标注
//!   「（事件流丢帧，档案不完整）」——档案宁可标注不完整，不静默缺字；
//! - `skill_triggered` / `confirm_request` / `async_tool_completed` / `error`
//!   不入库（通知类不进档案，2026-11-03 拍板口径）；`error` 不清空缓冲——
//!   其后若来 `turn_complete`，半截正文照实归档（如实原则）。

use robit_web::WebEvent;
use serde_json::json;

/// Web 会话在 `chat_messages.session` 列的固定键（通道独立：IM 会话历史归
/// robit 自持库，不进这张表——ADR-0011）。
pub const WEB_SESSION: &str = "web";

/// `/clear` 后 agent 的固定回执文本（robit-agent 硬编码；档案特判丢弃用）。
const CLEAR_ACK: &str = "[Conversation history cleared]";

/// 待写入行（调用方逐条 `Store::append_chat_message`，ts 由调用方盖）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowSpec {
    /// 工具轨迹展示快照 {name,arguments,result,is_error}（轮末兜底行 result=null）
    Tool { tool_json: String },
    /// 助手正文段（轮末定稿；含丢帧标注时已并入 text）
    Assistant { text: String },
}

/// 单轮 recorder 状态。`WebEvent::TurnComplete` 后全部字段复位。
#[derive(Default)]
pub struct RecorderCore {
    assistant: String,
    /// 本轮已请求、未收到结果的调用：Vec 保序（同轮多调用时档案顺序 = 事件顺序）
    pending: Vec<(String, String, String)>, // (tool_call_id, name, arguments)
    /// 本轮发生过 Lagged：定稿 assistant 段末尾追加如实标注
    incomplete: bool,
}

impl RecorderCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 消费一帧，返回需要落库的行（通常 0 或 1 条）。
    pub fn on_event(&mut self, ev: &WebEvent) -> Vec<RowSpec> {
        let mut out = Vec::new();
        match ev {
            WebEvent::TextDelta { text } => self.assistant.push_str(text),
            // 展示性瞬态：不入库（docs/02 口径）
            WebEvent::ThinkingDelta { .. } => {}
            WebEvent::ToolCall { tool_call_id, name, arguments } => {
                self.pending.push((tool_call_id.clone(), name.clone(), arguments.clone()));
            }
            WebEvent::ToolResult { tool_call_id, is_error, content } => {
                let pos = self.pending.iter().position(|(id, ..)| id == tool_call_id);
                let (name, args) = match pos {
                    Some(i) => {
                        let (_, n, a) = self.pending.remove(i);
                        (n, a)
                    }
                    // 迟到的结果对上已兜底落档的调用：照常成行（result 只在行里出现一次）
                    None => (String::new(), String::new()),
                };
                out.push(RowSpec::Tool {
                    tool_json: json!({
                        "name": name,
                        "arguments": args,
                        "result": content,
                        "is_error": is_error,
                    })
                    .to_string(),
                });
            }
            // 通知/确认/异常帧不进档案（error 不清缓冲：半截正文留给后续定稿）；
            // ScheduledTaskFired 为 0.8.0 新通知帧——Web 侧无计划任务接线（ADR-0022
            // 补记），正常不会出现，穷举 match 补臂归不入库组
            WebEvent::SkillTriggered { .. }
            | WebEvent::ConfirmRequest { .. }
            | WebEvent::AsyncToolCompleted { .. }
            | WebEvent::ScheduledTaskFired { .. }
            | WebEvent::Error { .. } => {}
            WebEvent::TurnComplete => {
                // 兜底：本轮结束仍无结果的调用（异常轮/被取消轮）按展示行落档，
                // result=null 如实表达「没拿到结果」
                for (_, name, args) in self.pending.drain(..) {
                    out.push(RowSpec::Tool {
                        tool_json: json!({
                            "name": name,
                            "arguments": args,
                            "result": null,
                            "is_error": true,
                        })
                        .to_string(),
                    });
                }
                if !self.assistant.is_empty() {
                    let mut text = std::mem::take(&mut self.assistant);
                    if text.trim() == CLEAR_ACK {
                        // /clear 回执丢弃；丢帧标注一并作废
                    } else {
                        if self.incomplete {
                            text.push_str("（事件流丢帧，档案不完整）");
                        }
                        out.push(RowSpec::Assistant { text });
                    }
                }
                self.incomplete = false;
            }
        }
        out
    }

    /// recorder 订阅端落后（broadcast Lagged）：当轮 assistant 定稿时如实标注。
    pub fn on_lagged(&mut self) {
        self.incomplete = true;
    }
}

/// 上海时刻 `YYYY-MM-DD HH:MM`（档案/留痕 ts；03 §9 生产时钟口径，与
/// holdings/watchlist 私有同款收敛到这一处）。
pub fn ts_now() -> String {
    zimu_market::clock::now_shanghai()
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use WebEvent::*;

    #[test]
    fn assistant_finalized_once_at_turn_complete() {
        let mut c = RecorderCore::new();
        assert!(c.on_event(&TextDelta { text: "结论：".into() }).is_empty());
        assert!(c.on_event(&TextDelta { text: "持有".into() }).is_empty());
        let rows = c.on_event(&TurnComplete);
        assert_eq!(rows, vec![RowSpec::Assistant { text: "结论：持有".into() }]);
        // 空轮不再产出幽灵行
        assert!(c.on_event(&TurnComplete).is_empty());
    }

    #[test]
    fn thinking_frames_never_persisted() {
        let mut c = RecorderCore::new();
        assert!(c.on_event(&ThinkingDelta { text: "思考中…".into() }).is_empty());
        assert!(c.on_event(&TurnComplete).is_empty(), "只有思考帧的轮次没有档案行");
    }

    #[test]
    fn tool_call_result_pair_merges_to_one_row() {
        let mut c = RecorderCore::new();
        c.on_event(&ToolCall {
            tool_call_id: "t1".into(),
            name: "get_quote".into(),
            arguments: r#"{"codes":["sh600519"]}"#.into(),
        });
        let rows = c.on_event(&ToolResult {
            tool_call_id: "t1".into(),
            is_error: false,
            content: "{\"sh600519\":...}".into(),
        });
        assert_eq!(rows.len(), 1);
        let RowSpec::Tool { tool_json } = &rows[0] else { panic!("应为 tool 行") };
        let v: serde_json::Value = serde_json::from_str(tool_json).unwrap();
        assert_eq!(v["name"], "get_quote");
        assert_eq!(v["is_error"], false);
        assert!(v["result"].as_str().unwrap().contains("600519"));
        // 结果已消费：turn_complete 不重复兜底
        assert!(c.on_event(&TurnComplete).is_empty());
    }

    #[test]
    fn unanswered_tool_call_floors_with_null_result() {
        let mut c = RecorderCore::new();
        c.on_event(&ToolCall {
            tool_call_id: "t9".into(),
            name: "refresh_quotes".into(),
            arguments: "{}".into(),
        });
        let rows = c.on_event(&TurnComplete);
        assert_eq!(rows.len(), 1);
        let RowSpec::Tool { tool_json } = &rows[0] else { panic!("应为兜底 tool 行") };
        let v: serde_json::Value = serde_json::from_str(tool_json).unwrap();
        assert!(v["result"].is_null(), "无结果的调用如实 result=null");
        assert_eq!(v["is_error"], true);
    }

    #[test]
    fn lagged_marks_assistant_incomplete() {
        let mut c = RecorderCore::new();
        c.on_event(&TextDelta { text: "前半".into() });
        c.on_lagged();
        c.on_event(&TextDelta { text: "后半".into() });
        let rows = c.on_event(&TurnComplete);
        assert!(matches!(&rows[0], RowSpec::Assistant { text } if text.ends_with("（事件流丢帧，档案不完整）")));
        // 标注一次性：下一轮不带标注
        c.on_event(&TextDelta { text: "新轮".into() });
        let rows = c.on_event(&TurnComplete);
        assert_eq!(rows, vec![RowSpec::Assistant { text: "新轮".into() }]);
    }

    #[test]
    fn clear_ack_is_dropped() {
        let mut c = RecorderCore::new();
        c.on_event(&TextDelta { text: CLEAR_ACK.into() });
        assert!(c.on_event(&TurnComplete).is_empty(), "/clear 固定回执不进档案");
    }

    #[test]
    fn notice_frames_do_not_persist() {
        let mut c = RecorderCore::new();
        c.on_event(&SkillTriggered { name: "post-market-review".into(), description: "d".into() });
        c.on_event(&ConfirmRequest {
            tool_call_id: "x".into(),
            name: "n".into(),
            arguments: "{}".into(),
        });
        c.on_event(&AsyncToolCompleted { task_id: "a".into(), tool_call_id: "x".into(), is_error: true });
        c.on_event(&Error { message: "上游 500".into() });
        assert!(c.on_event(&TurnComplete).is_empty());
    }

    #[test]
    fn error_mid_turn_keeps_partial_text_for_final_flush() {
        // 半截正文 + Error + TurnComplete：正文如实归档（半截也是模型真实输出）
        let mut c = RecorderCore::new();
        c.on_event(&TextDelta { text: "看到一半".into() });
        c.on_event(&Error { message: "连接中断".into() });
        let rows = c.on_event(&TurnComplete);
        assert_eq!(rows, vec![RowSpec::Assistant { text: "看到一半".into() }]);
    }
}
