//! Request-scoped tool identity, choice and result projection shared by wire adapters.
use crate::ir::{
    CallId, IrError, ItemId, ToolIdentity, ToolKind,
    bridge::CustomToolBridge,
    request::{Extensions, RequestIR, ToolCall, ToolCallStatus, ToolChoice, ToolInput},
};
use serde_json::{Value, json};

pub(crate) struct PreparedTools {
    pub(crate) registry: CustomToolBridge,
    parallel: Option<bool>,
    choice: Option<ToolChoice>,
}
impl PreparedTools {
    pub(crate) fn new(registry: CustomToolBridge, request: &RequestIR) -> Self {
        Self {
            registry,
            parallel: request.generation.parallel_tool_calls,
            choice: request.generation.tool_choice.clone(),
        }
    }
    pub(crate) fn resolve(&self, alias: &str) -> Result<(&ToolIdentity, ToolKind), IrError> {
        self.resolve_identity(&ToolIdentity::new(None, alias)?)
    }
    pub(crate) fn resolve_identity(
        &self,
        alias: &ToolIdentity,
    ) -> Result<(&ToolIdentity, ToolKind), IrError> {
        let result = self.registry.original(alias)?;
        if matches!(&self.choice, Some(ToolChoice::None))
            || matches!(&self.choice, Some(ToolChoice::Named { tool, .. }) if tool != result.0)
        {
            return Err(IrError::InvalidToolMapping);
        }
        Ok(result)
    }
    pub(crate) fn validate_count(&self, count: usize, completed: bool) -> Result<(), IrError> {
        if (self.parallel == Some(false) && count > 1)
            || (completed
                && count == 0
                && matches!(
                    &self.choice,
                    Some(ToolChoice::Required | ToolChoice::Named { .. })
                ))
        {
            return Err(IrError::InvalidToolMapping);
        }
        Ok(())
    }
    pub(crate) fn restore(
        &self,
        name: &str,
        call_id: CallId,
        item: ItemId,
        raw: String,
    ) -> Result<ToolCall, IrError> {
        let call = self.registry.restore_call(&ToolCall {
            status: Some(ToolCallStatus::Completed),
            item_id: Some(item),
            call_id,
            tool: ToolIdentity::new(None, name)?,
            input: ToolInput::Json(raw),
            extensions: Extensions::responses(),
        })?;
        self.resolve(name)?;
        Ok(call)
    }
}
pub(crate) fn tool_output(call: &ToolCall, status: &str) -> Value {
    let mut item = json!({"id":call.item_id.as_ref().expect("output item id").as_str(), "status":status,
        "call_id":call.call_id.as_str(),"name":call.tool.name});
    if let Some(namespace) = &call.tool.namespace {
        item["namespace"] = json!(namespace);
    }
    match &call.input {
        ToolInput::Json(raw) => {
            item["type"] = json!("function_call");
            item["arguments"] = json!(raw);
        }
        ToolInput::Freeform(raw) => {
            item["type"] = json!("custom_tool_call");
            item["input"] = json!(raw);
        }
    }
    item
}
