pub mod cloud;
pub mod ollama;

use crate::models::{GeneratedBatch, Settings, Translation};
use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;

/// 翻译词的系统提示词。要求模型只吐 JSON，前端直接渲染结构化字段。
pub const PROMPT_TRANSLATE: &str = r#"你是一本质量很高的英汉词典，面向中文母语的技术从业者。

用户会给你一段从屏幕上选中的英文文本，它可能是单词、短语或整句。

只输出一个 JSON 对象，不要 markdown 代码块，不要任何解释文字：

{
  "word": "要解释的词或短语（小写形式；若输入是整句，取其中最核心的那个词）",
  "phonetic": "英式音标，用斜杠包住，例如 /ˈɔːɡənaɪz/；拿不准就留空字符串",
  "pos": "词性缩写，例如 v. / n. / adj. / adv.；短语可留空",
  "definition": "一句中文说清核心含义。先给最常用的意思，必要时补一句易混淆点的辨析。不超过 80 字，不要罗列六七个义项。",
  "collocations": [
    {"en": "最高频的搭配 1", "zh": "中文"},
    {"en": "最高频的搭配 2", "zh": "中文"}
  ],
  "example": {"en": "英文例句", "zh": "中文翻译"}
}

规则：
1. definition 要像人给同事解释，不要词典腔。例如「整理、组织 —— 把零散的东西归拢成一个有序整体」。
2. collocations 给 2 到 3 个，必须是真的高频搭配。
3. example 优先直接用用户给你的原句（如果它本身是完整句子）；否则自己造一个贴合程序员/技术文档语境的句子。
4. 如果输入已经是中文，word 字段填原文，definition 用中文解释它在英文里通常对应什么词。
5. 严格输出 JSON，第一个字符必须是 {，最后一个字符必须是 }。"#;

/// 出复习题的系统提示词
pub const PROMPT_QUIZ: &str = r#"你是一个英语复习出题老师，面向一个中文母语的技术从业者。

用户会给你一批生词，每个词带音标、释义和他当初遇到这个词的原文语境。

请为每个词出一道「在句子里猜词义」的单选题，只输出 JSON，不要任何解释文字：

{
  "questions": [
    {
      "word": "被考的词",
      "sentence": "一个全新的英文句子，把被考词嵌进去，该词用 **双星号** 包裹",
      "sentence_zh": "这句话的中文翻译",
      "options": ["错误选项1", "正确选项2", "错误选项3", "错误选项4"],
      "answer_index": 1,
      "explanation": "中文解析：为什么是这个词义，以及最容易和哪个词混淆"
    }
  ]
}

规则：
1. 句子必须是全新的，不要照抄用户给的原文语境，但要贴近他熟悉的领域（编程、技术文档、工具使用）。
2. 四个选项都是中文短释义，长度接近（4 到 12 个字），干扰项要像真的——优先用该词的其他常见义项，或者形近词的意思。
3. answer_index 是正确答案在 options 里的下标，从 0 开始。
4. explanation 两句话以内，讲清辨析点。
5. 句子里的被考词必须用 **双星号** 单独包裹，方便前端高亮。
6. questions 的数量和输入词数一致，严格输出 JSON。"#;

pub async fn translate(text: &str, settings: &Settings) -> Result<Translation> {
    let raw = call(settings, PROMPT_TRANSLATE, text).await?;
    parse::<Translation>(&raw)
}

pub async fn generate_quiz(words_json: &str, settings: &Settings) -> Result<GeneratedBatch> {
    let raw = call(settings, PROMPT_QUIZ, words_json).await?;
    parse::<GeneratedBatch>(&raw)
}

async fn call(settings: &Settings, system: &str, user: &str) -> Result<String> {
    match settings.engine.as_str() {
        "ollama" => ollama::chat(settings, system, user).await,
        _ => cloud::chat(settings, system, user).await,
    }
}

/// 模型有时候会包 ```json 或者前后加寒暄，这里把 JSON 主体抠出来
fn parse<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let slice = extract_json(raw);
    serde_json::from_str::<T>(slice).map_err(|e| {
        anyhow!(
            "模型返回的不是合法 JSON（{}）。原始返回前 300 字：{}",
            e,
            truncate(slice, 300)
        )
    })
}

fn extract_json(raw: &str) -> &str {
    let start = raw.find('{');
    let end = raw.rfind('}');
    match (start, end) {
        (Some(s), Some(e)) if e > s => &raw[s..=e],
        _ => raw,
    }
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let head: String = s.chars().take(n).collect();
        format!("{}…", head)
    }
}
