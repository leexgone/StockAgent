//! echo —— 链路验证工具（P2-T2 验收项）。
//!
//! 原 zimu-server agent_bridge 与 zimu-agent-cli 各有一份（spike 期容忍的重复），
//! P4-T6 收敛到这里。领域工具（tools/）成形后它仍保留：网关/模型连通性排查时
//! 不依赖行情与持仓数据即可验证 function-calling 全链路。

use async_trait::async_trait;
use robit_agent::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "原样回显一段文本。这是子木助理的链路验证工具：当用户要求回显、复读某段文字时，必须调用本工具，不要直接回答。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "要回显的文本" }
            },
            "required": ["text"]
        })
    }

    fn requires_confirmation(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
    ) -> robit_agent::error::Result<ToolResult> {
        let text = args.get("text").and_then(Value::as_str).unwrap_or("");
        Ok(ToolResult::success(format!("[子木 echo] {text}")))
    }
}
