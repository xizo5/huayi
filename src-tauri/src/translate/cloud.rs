use crate::models::Settings;
use crate::translate::truncate;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

/// OpenAI 兼容的 chat/completions 接口。
/// DeepSeek、Moonshot、OpenAI、以及各种中转站都是这个形状。
pub async fn chat(s: &Settings, system: &str, user: &str) -> Result<String> {
    let key = s.cloud_api_key.trim();
    if key.is_empty() {
        return Err(anyhow!(
            "还没填云端 API Key。打开设置填一个，或者把引擎切成本地 Ollama。"
        ));
    }

    let url = format!("{}/chat/completions", s.cloud_base_url.trim_end_matches('/'));

    let body = json!({
        "model": s.cloud_model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "temperature": 0.2,
        "stream": false
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()?;

    let resp = client
        .post(&url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("连不上 {}：{}", url, e))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow!(
            "云端返回 {}。响应：{}",
            status.as_u16(),
            truncate(&text, 300)
        ));
    }

    let v: Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("响应不是 JSON：{}", truncate(&text, 300)))?;

    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("响应里没有 choices[0].message.content：{}", truncate(&text, 300)))
}

/// 连通性自检，设置页的「测试」按钮用
pub async fn ping(s: &Settings) -> Result<String> {
    let raw = chat(
        s,
        "你是一个连通性测试。只回复 JSON：{\"ok\":true}",
        "ping",
    )
    .await?;
    Ok(truncate(&raw, 120))
}
