use serde::{Deserialize, Serialize};

/// 常见搭配
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Collocation {
    #[serde(default)]
    pub en: String,
    #[serde(default)]
    pub zh: String,
}

/// 例句
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Example {
    #[serde(default)]
    pub en: String,
    #[serde(default)]
    pub zh: String,
}

/// 一次翻译的结构化结果，由 LLM 直接产出 JSON
///
/// 有两种形态，由 `kind` 区分：
/// - `word`：词典式结果（词头、音标、释义、搭配、例句）
/// - `sentence`：整句翻译（原文 + 译文）
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Translation {
    /// "word" | "sentence"
    #[serde(default)]
    pub kind: String,
    /// 句子模式下的原文
    #[serde(default)]
    pub source: String,
    /// 句子模式下的整句译文
    #[serde(default)]
    pub sentence_zh: String,

    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub phonetic: String,
    #[serde(default)]
    pub pos: String,
    #[serde(default)]
    pub definition: String,
    #[serde(default)]
    pub collocations: Vec<Collocation>,
    #[serde(default)]
    pub example: Option<Example>,
}

/// 生词本条目
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Word {
    pub id: String,
    pub word: String,
    pub phonetic: String,
    pub pos: String,
    pub definition: String,
    /// 用户遇到这个词时的原文语境
    pub context: String,
    pub created_at: i64,
    /// 熟悉度 0..=5，越高越熟
    pub familiarity: i64,
    pub reviewed_at: Option<i64>,
}

/// 预生成的一道复习题
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReviewItem {
    pub id: String,
    pub word: String,
    pub sentence: String,
    pub sentence_zh: String,
    pub options: Vec<String>,
    pub answer_index: i64,
    pub explanation: String,
    pub answered: bool,
    pub chosen: Option<i64>,
    pub correct: Option<bool>,
}

/// LLM 出的题（原始结构）
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GeneratedQuestion {
    #[serde(default)]
    pub word: String,
    #[serde(default)]
    pub sentence: String,
    #[serde(default)]
    pub sentence_zh: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub answer_index: i64,
    #[serde(default)]
    pub explanation: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GeneratedBatch {
    #[serde(default)]
    pub questions: Vec<GeneratedQuestion>,
}

/// 应用设置
///
/// 容器级 `serde(default)` 是必须的：以后加字段时旧数据还能读出来，
/// 否则反序列化整体失败会回退到 Default，把用户填的 API Key 一起清掉。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// cloud | ollama
    pub engine: String,
    pub cloud_base_url: String,
    pub cloud_api_key: String,
    pub cloud_model: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
    /// 划选后是否自动浮出翻译入口
    pub auto_popup: bool,
    /// 每日复习题量
    pub daily_limit: i64,
    /// 触发拖选的最小位移（像素），过滤误触
    pub drag_threshold: f64,
    /// 唤出翻译的全局快捷键，形如 "Alt+D"；空字符串表示关闭
    pub hotkey: String,
    /// 快捷键总开关。关掉后组合键失效，但配置保留
    pub hotkey_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: "cloud".into(),
            cloud_base_url: "https://api.deepseek.com/v1".into(),
            cloud_api_key: String::new(),
            cloud_model: "deepseek-chat".into(),
            ollama_base_url: "http://127.0.0.1:11434".into(),
            ollama_model: "qwen2.5:7b".into(),
            auto_popup: true,
            daily_limit: 8,
            drag_threshold: 12.0,
            hotkey: "Alt+D".into(),
            hotkey_enabled: true,
        }
    }
}
