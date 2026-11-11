use crate::models::Settings;
use crate::translate::truncate;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

/// 本地 Ollama 的 /api/chat。用 format=json 强制结构化输出。
pub async fn chat(s: &Settings, system: &str, user: &str) -> Result<String> {
    let url = format!("{}/api/chat", s.ollama_base_url.trim_end_matches('/'));

    let body = json!({
        "model": s.ollama_model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false,
        "format": "json",
        "options": { "temperature": 0.2 }
    });

    // 本地模型首 token 可能很慢，给足时间
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "连不上 Ollama（{}）。确认 ollama serve 在跑，或者模型 {} 已经 pull 过。",
                e,
                s.ollama_model
            )
        })?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(anyhow!(
            "Ollama 返回 {}。响应：{}",
            status.as_u16(),
            truncate(&text, 300)
        ));
    }

    let v: Value = serde_json::from_str(&text)
        .map_err(|_| anyhow!("Ollama 响应不是 JSON：{}", truncate(&text, 300)))?;

    v["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Ollama 响应里没有 message.content：{}", truncate(&text, 300)))
}

pub async fn ping(s: &Settings) -> Result<String> {
    let url = format!("{}/api/tags", s.ollama_base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let resp = client.get(&url).send().await.map_err(|e| {
        anyhow!(
            "连不上 Ollama（{}）。Windows 上确认 ollama 服务已启动。",
            e
        )
    })?;
    if !resp.status().is_success() {
        return Err(anyhow!("Ollama 返回 {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await?;
    let models: Vec<String> = v["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(format!("已连上，本地有 {} 个模型：{}", models.len(), models.join(", ")))
}
