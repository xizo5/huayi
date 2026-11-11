pub mod cloud;
pub mod ollama;

use crate::models::{GeneratedBatch, Settings, Translation};
use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;

/// 单词/短语：词典式释义。只在输入是「短单位」时使用。
pub const PROMPT_TRANSLATE: &str = r#"你是一本质量很高的英汉词典，面向中文母语的技术从业者。

用户会给你一个英文词或短语。请解释它。

只输出一个 JSON 对象，不要 markdown 代码块，不要任何解释文字：

{
  "kind": "word",
  "word": "要解释的词或短语（小写形式）",
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
3. example 造一个贴合程序员/技术文档语境的句子。
4. 严格输出 JSON，第一个字符必须是 {，最后一个字符必须是 }。"#;

/// 整句：直译成通顺中文。和上面那个 prompt 是两种活，别混用。
pub const PROMPT_SENTENCE: &str = r#"你是一个英译中翻译器，面向中文母语的技术从业者。

用户会给你一段英文句子或段落，请把它翻译成自然、通顺的中文。

只输出一个 JSON 对象，不要 markdown 代码块，不要任何解释文字：

{
  "kind": "sentence",
  "source": "原文，一字不改地返回",
  "sentence_zh": "中文译文"
}

规则：
1. 译文要像中文母语者写的，不要翻译腔。英文的长定语从句拆成短中文短句，不要保留英文语序。
2. **专有名词一律保留英文原样**：人名、产品名、公司名、框架名、命令、代码标识符都不要音译或意译。
   例如 Claude、GitHub、Kubernetes、VS Code、commit、prompt、token 都照抄，不要写成"克劳德""提交"这种。
3. 祈使句用祈使语气，疑问句用疑问语气。
4. 不要加任何解释、注释或"这句话的意思是"之类的引导语，只要译文。
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
    let prompt = if looks_like_sentence(text) {
        PROMPT_SENTENCE
    } else {
        PROMPT_TRANSLATE
    };
    let raw = call(settings, prompt, text).await?;
    let mut t = parse::<Translation>(&raw)?;

    // 模型偶尔忘填 kind，这里按实际走的分支兜底
    if t.kind.is_empty() {
        t.kind = if looks_like_sentence(text) {
            "sentence".into()
        } else {
            "word".into()
        };
    }
    if t.kind == "sentence" && t.source.is_empty() {
        t.source = text.trim().to_string();
    }

    Ok(t)
}

/// 判断输入该走「整句翻译」还是「词典释义」。
///
/// 之前这里只有一条 prompt，里面写着"若输入是整句，取最核心的词" —— 那是为划词
/// 设计的（选一段代码里的英文，通常只想知道那个词）。但翻译页粘整句时它也照做，
/// 结果整句被拆成一个词，用户看到的就是"只翻了第一个词"。
fn looks_like_sentence(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let words = t.split_whitespace().count();
    words >= 4
        || t.ends_with('.')
        || t.ends_with('?')
        || t.ends_with('!')
        || t.ends_with('。')
        || t.ends_with('？')
        || t.ends_with('！')
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
