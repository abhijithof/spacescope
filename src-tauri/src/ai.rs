//! Optional "ask local AI" for items the knowledge base doesn't recognise. Uses Ollama on
//! this PC only — nothing about your files leaves the machine.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

const OLLAMA: &str = "http://127.0.0.1:11434";

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiAnswer {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub what: String,
    #[serde(default)]
    pub important: String,
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub reason: String,
}

fn pick_model(client: &reqwest::blocking::Client) -> Result<String, String> {
    let tags: serde_json::Value = client
        .get(format!("{OLLAMA}/api/tags"))
        .send()
        .map_err(|_| "Ollama isn't running. Start Ollama to use AI explanations.".to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = tags["models"].as_array().into_iter().flatten().filter_map(|m| m["name"].as_str().map(String::from)).collect();
    for pref in ["qwen3", "qwen2.5", "llama3", "gemma3", "gemma", "mistral", "phi"] {
        if let Some(n) = names.iter().find(|n| n.starts_with(pref) && !n.contains("embed")) {
            return Ok(n.clone());
        }
    }
    names.into_iter().find(|n| !n.contains("embed")).ok_or_else(|| "No Ollama models installed (try `ollama pull qwen3:8b`).".into())
}

pub fn explain(context: &str) -> Result<AiAnswer, String> {
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(180)).build().map_err(|e| e.to_string())?;
    let model = pick_model(&client)?;
    let system = "You are a Windows storage expert helping a user clean their disk. Given a file or folder on a Windows PC, \
explain what it most likely is. Be concrete: name the app or Windows component that created it when the path makes it clear. \
If you are not sure, say so plainly instead of guessing. Answer ONLY with JSON: \
{\"what\": \"1-3 sentences: what this is and which program created it\", \
\"important\": \"1-2 sentences: what it's needed for / what breaks or is lost if deleted\", \
\"verdict\": \"safe\" | \"caution\" | \"keep\" | \"system\", \
\"reason\": \"one sentence justifying the verdict\"}. \
safe = deleting loses nothing (cache/temp/logs). caution = deletable but you lose something (settings, re-download). \
keep = personal data or an installed app. system = Windows needs it.";
    let body = json!({
        "model": model,
        "stream": false,
        "think": false,
        "format": "json",
        "options": { "temperature": 0.2 },
        "messages": [ { "role": "system", "content": system }, { "role": "user", "content": context } ]
    });
    let resp: serde_json::Value = client.post(format!("{OLLAMA}/api/chat")).json(&body).send().map_err(|e| e.to_string())?.json().map_err(|e| e.to_string())?;
    let mut text = resp["message"]["content"].as_str().unwrap_or("").to_string();
    if let Some(end) = text.find("</think>") {
        text = text[end + 8..].to_string();
    }
    let start = text.find('{').unwrap_or(0);
    let end = text.rfind('}').map(|e| e + 1).unwrap_or(text.len());
    let mut a: AiAnswer = serde_json::from_str(&text[start..end]).map_err(|_| format!("The model gave an unreadable answer: {}", text.trim()))?;
    a.model = model;
    a.verdict = a.verdict.to_lowercase();
    Ok(a)
}
