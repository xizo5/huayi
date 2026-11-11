use crate::db::Db;
use crate::models::{ReviewItem, Settings, Translation, Word};
use crate::translate;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------- 翻译 ----------------

#[derive(Serialize)]
pub struct TranslateResult {
    pub translation: Translation,
    pub saved: bool,
}

#[tauri::command]
pub async fn translate(app: AppHandle, text: String) -> R<TranslateResult> {
    let settings = app.state::<Db>().load_settings();

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("没有可翻译的内容".into());
    }
    if trimmed.chars().count() > 1500 {
        return Err("选中的文本太长了（超过 1500 字）".into());
    }

    let translation = translate::translate(&trimmed, &settings)
        .await
        .map_err(err)?;

    let saved = app
        .state::<Db>()
        .exists(&translation.word.to_lowercase())
        .unwrap_or(false);

    Ok(TranslateResult { translation, saved })
}

// ---------------- 生词本 ----------------

#[derive(Deserialize)]
pub struct AddWordInput {
    pub word: String,
    pub phonetic: String,
    pub pos: String,
    pub definition: String,
    pub context: String,
}

#[tauri::command]
pub fn add_word(app: AppHandle, input: AddWordInput) -> R<Word> {
    let db = app.state::<Db>();
    let w = Word {
        id: uuid::Uuid::new_v4().to_string(),
        word: input.word.trim().to_lowercase(),
        phonetic: input.phonetic,
        pos: input.pos,
        definition: input.definition,
        context: input.context,
        created_at: chrono::Utc::now().timestamp(),
        familiarity: 0,
        reviewed_at: None,
    };
    db.add_word(&w).map_err(err)
}

#[tauri::command]
pub fn list_words(app: AppHandle, query: Option<String>) -> R<Vec<Word>> {
    app.state::<Db>()
        .list_words(query.as_deref(), 500)
        .map_err(err)
}

#[tauri::command]
pub fn delete_word(app: AppHandle, id: String) -> R<()> {
    app.state::<Db>().delete_word(&id).map_err(err)
}

#[derive(Serialize)]
pub struct Stats {
    pub total: i64,
    pub mastered: i64,
    pub pending_review: i64,
}

#[tauri::command]
pub fn stats(app: AppHandle) -> R<Stats> {
    let db = app.state::<Db>();
    Ok(Stats {
        total: db.word_count().unwrap_or(0),
        mastered: db.mastered_count().unwrap_or(0),
        pending_review: db.pending_review_count().unwrap_or(0),
    })
}

// ---------------- 设置 ----------------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<Db>().load_settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> R<()> {
    app.state::<Db>().save_settings(&settings).map_err(err)?;
    // 快捷键改了要立刻生效，不用重启
    crate::selection::apply_hotkey(&settings.hotkey, settings.hotkey_enabled);
    Ok(())
}

#[tauri::command]
pub async fn test_engine(app: AppHandle) -> R<String> {
    let s = app.state::<Db>().load_settings();
    match s.engine.as_str() {
        "ollama" => translate::ollama::ping(&s).await.map_err(err),
        _ => translate::cloud::ping(&s).await.map_err(err),
    }
}

// ---------------- 复习 ----------------

/// 预生成复习题。已有足够待答题就直接返回 0，不浪费 token。
#[tauri::command]
pub async fn prepare_review(app: AppHandle) -> R<usize> {
    let (settings, need, words) = {
        let db = app.state::<Db>();
        let settings = db.load_settings();
        let pending = db.pending_review_count().unwrap_or(0);
        if pending >= settings.daily_limit {
            return Ok(0);
        }
        let need = settings.daily_limit - pending;

        let mut words = db.words_for_generation(need).unwrap_or_default();
        if words.is_empty() {
            words = db.words_for_regeneration(need).unwrap_or_default();
        }
        (settings, need, words)
    };

    if words.is_empty() {
        return Ok(0);
    }

    let payload: Vec<serde_json::Value> = words
        .iter()
        .take(need as usize)
        .map(|w| {
            serde_json::json!({
                "word": w.word,
                "phonetic": w.phonetic,
                "definition": w.definition,
                "context": w.context,
            })
        })
        .collect();

    let payload = serde_json::to_string(&payload).map_err(err)?;
    let batch = translate::generate_quiz(&payload, &settings)
        .await
        .map_err(err)?;

    let known_words: Vec<String> = words.iter().map(|w| w.word.clone()).collect();

    let items: Vec<ReviewItem> = batch
        .questions
        .into_iter()
        .filter(|q| {
            q.options.len() >= 2
                && q.answer_index >= 0
                && (q.answer_index as usize) < q.options.len()
                && !q.sentence.trim().is_empty()
        })
        .map(|q| ReviewItem {
            id: uuid::Uuid::new_v4().to_string(),
            word: if q.word.is_empty() {
                known_words.first().cloned().unwrap_or_default()
            } else {
                q.word.to_lowercase()
            },
            sentence: q.sentence,
            sentence_zh: q.sentence_zh,
            options: q.options,
            answer_index: q.answer_index,
            explanation: q.explanation,
            answered: false,
            chosen: None,
            correct: None,
        })
        .collect();

    let n = items.len();
    let db = app.state::<Db>();
    db.insert_questions(&items).map_err(err)?;
    let _ = db.prune_answered();

    Ok(n)
}

#[tauri::command]
pub fn get_review(app: AppHandle) -> R<Vec<ReviewItem>> {
    let db = app.state::<Db>();
    let limit = db.load_settings().daily_limit;
    db.pending_questions(limit).map_err(err)
}

#[derive(Serialize)]
pub struct AnswerResult {
    pub correct: bool,
    pub answer_index: i64,
    pub explanation: String,
}

#[tauri::command]
pub fn answer_question(app: AppHandle, id: String, chosen: i64) -> R<AnswerResult> {
    let db = app.state::<Db>();
    let correct = db.answer(&id, chosen).map_err(err)?;

    let items = db.pending_questions(1000).unwrap_or_default();
    let explanation = items
        .iter()
        .find(|i| i.id == id)
        .map(|i| i.explanation.clone())
        .unwrap_or_default();
    let answer_index = items
        .iter()
        .find(|i| i.id == id)
        .map(|i| i.answer_index)
        .unwrap_or(-1);

    Ok(AnswerResult {
        correct,
        answer_index,
        explanation,
    })
}

// ---------------- 窗口 ----------------

#[tauri::command]
pub fn show_main(app: AppHandle, view: Option<String>) {
    if let Some(w) = app.get_webview_window("main") {
        // SW_RESTORE：主窗口可能已经被最小化或收进托盘，光 show 不还原
        crate::platform::show_and_focus(&w);
        if let Some(v) = view {
            let _ = w.emit("goto", v);
        }
    }
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    crate::popup::close(&app);
}
