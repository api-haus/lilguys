//! One blocking client for every OpenAI-compatible endpoint. Providers differ only by base URL.

use crate::config::Provider;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Why generation stopped, and how much it spent. Never sent back, only reported.
    #[serde(default, skip_serializing)]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing)]
    pub completion_tokens: u64,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self::plain("system", text)
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::plain("user", text)
    }

    pub fn plain(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            finish_reason: None,
            completion_tokens: 0,
        }
    }

    pub fn tool_result(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: Some(id.into()),
            finish_reason: None,
            completion_tokens: 0,
        }
    }

    /// Four characters to a token is close enough to schedule compaction by.
    pub fn approx_tokens(&self) -> usize {
        let body = self.content.as_deref().map(str::len).unwrap_or(0);
        let calls = self
            .tool_calls
            .as_ref()
            .map(|c| c.iter().map(|c| c.function.arguments.len() + 24).sum::<usize>())
            .unwrap_or(0);
        (body + calls) / 4 + 4
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(default = "call_kind")]
    pub r#type: String,
    pub function: FunctionCall,
}

fn call_kind() -> String {
    "function".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}

/// Where a local backend usually answers. Probed before anything hosted is ever suggested.
pub const LOCAL: [(&str, &str); 3] = [
    ("ollama", "http://127.0.0.1:11434/v1"),
    ("llamacpp", "http://127.0.0.1:8080/v1"),
    ("lmstudio", "http://127.0.0.1:1234/v1"),
];

/// What an endpoint says it can run. Also the cheapest possible reachability test.
pub fn models(url: &str, key: Option<&str>, timeout: std::time::Duration) -> Result<Vec<String>> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(timeout)).build().into();
    let mut req = agent.get(&format!("{}/models", url.trim_end_matches('/')));
    if let Some(key) = key {
        req = req.header("authorization", &format!("Bearer {key}"));
    }
    let mut res = req.call().context("no answer")?;
    let parsed: Value = res.body_mut().read_json().context("the reply was not json")?;
    let mut names: Vec<String> = parsed
        .get("data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).map(str::to_string).collect()
        })
        .unwrap_or_default();
    names.sort();
    Ok(names)
}

pub struct Client {
    pub provider: Provider,
    agent: ureq::Agent,
    key: Option<String>,
}

impl Client {
    pub fn new(provider: Provider) -> Result<Self> {
        let key = match provider.api_key_env.as_ref() {
            Some(var) => Some(
                std::env::var(var)
                    .with_context(|| format!("{var} is not set, but the provider wants a key"))?,
            ),
            None => None,
        };
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(provider.timeout))
            .build()
            .into();
        Ok(Self { provider, agent, key })
    }


    pub fn chat(&self, messages: &[Message], tools: &Value) -> Result<Message> {
        let mut body = json!({
            "model": self.provider.model,
            "messages": messages,
            "temperature": self.provider.temperature,
            "max_tokens": self.provider.max_tokens,
        });
        if !tools.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            body["tools"] = tools.clone();
            body["tool_choice"] = json!("auto");
        }
        for (k, v) in &self.provider.extra {
            if let Ok(v) = serde_json::to_value(v) {
                body[k] = v;
            }
        }

        if std::env::var_os("LILGUYS_DEBUG_HTTP").is_some() {
            let mut shown = body.clone();
            shown["messages"] = json!(format!("<{} messages>", messages.len()));
            shown["tools"] = json!(format!("<{} tools>", tools.as_array().map(Vec::len).unwrap_or(0)));
            eprintln!("http request: {shown}");
        }

        let url = format!("{}/chat/completions", self.provider.url.trim_end_matches('/'));
        let mut req = self.agent.post(&url).header("content-type", "application/json");
        if let Some(key) = self.key.as_deref() {
            req = req.header("authorization", &format!("Bearer {key}"));
        }
        for (k, v) in &self.provider.headers {
            req = req.header(k, v);
        }

        let mut res = req.send_json(&body).context("chat request failed")?;
        let parsed: Value = res.body_mut().read_json().context("chat response was not json")?;
        if std::env::var_os("LILGUYS_DEBUG_HTTP").is_some() {
            eprintln!("http reply  : {}", &parsed.to_string()[..parsed.to_string().len().min(600)]);
        }
        if let Some(err) = parsed.get("error") {
            bail!("provider error: {err}");
        }
        let first = parsed
            .get("choices")
            .and_then(|c| c.get(0))
            .context("chat response had no choices")?;
        let mut message: Message = serde_json::from_value(
            first.get("message").context("chat response had no message")?.clone(),
        )
        .context("could not read the assistant message")?;
        message.finish_reason =
            first.get("finish_reason").and_then(Value::as_str).map(str::to_string);
        message.completion_tokens = parsed
            .get("usage")
            .and_then(|u| u.get("completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        Ok(message)
    }

    /// A plain completion with no tools, used for context compaction.
    pub fn summarise(&self, messages: &[Message]) -> Result<String> {
        let reply = self.chat(messages, &json!([]))?;
        Ok(reply.content.unwrap_or_default())
    }
}
