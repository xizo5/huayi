const { invoke } = window.__TAURI__.core;

const main = document.getElementById('main');
const nav = document.getElementById('nav');

const esc = (s) =>
  String(s ?? '').replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
  );

const hl = (s) => esc(s).replace(/\*\*(.+?)\*\*/g, '<mark>$1</mark>');

let view = 'translate';
let searchTerm = '';
let settingsCache = null;
let hotkeyValue = '';

// 把 KeyboardEvent.key 翻译成后端 map_key 认识的名字
function keyNameOf(k) {
  if (k.length === 1 && /[a-zA-Z0-9]/.test(k)) return k.toUpperCase();
  if (/^F([1-9]|1[0-2])$/i.test(k)) return k.toUpperCase();
  const map = {
    ' ': 'Space',
    Enter: 'Enter',
    Tab: 'Tab',
    Escape: 'Esc',
    Backspace: 'Backspace',
    Delete: 'Delete',
    Insert: 'Insert',
    Home: 'Home',
    End: 'End',
    PageUp: 'PageUp',
    PageDown: 'PageDown',
    ArrowUp: 'Up',
    ArrowDown: 'Down',
    ArrowLeft: 'Left',
    ArrowRight: 'Right',
  };
  return map[k] || null;
}

// 复习会话状态
const review = { items: [], idx: 0, answered: false, chosen: -1, result: null, results: [] };

// ---------------- 路由 ----------------

nav.addEventListener('click', (e) => {
  const b = e.target.closest('button[data-view]');
  if (!b) return;
  view = b.dataset.view;
  nav.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b));
  render();
});

async function render() {
  if (view === 'translate') return renderTranslate();
  if (view === 'vocab') return renderVocab();
  if (view === 'review') return renderReview();
  return renderSettings();
}

async function refreshBadges() {
  try {
    const s = await invoke('stats');
    document.getElementById('navCount').textContent = s.total;
    document.getElementById('navDue').textContent = s.pending_review;
  } catch (_) {}

  // 侧边栏显示实际配置的快捷键，而不是写死的 Alt+D
  try {
    const cfg = await invoke('get_settings');
    const kbd = document.getElementById('sideHotkey');
    if (kbd) {
      kbd.textContent = cfg.hotkey_enabled ? cfg.hotkey || '未设置' : '已停用';
    }
  } catch (_) {}
}

// ---------------- 翻译 ----------------

// 独立翻译页：不依赖划词、不依赖全局快捷键，永远不会因为被别的软件占用而失效
let tInput = '';
let tResult = null;
let tLoading = false;
let tError = '';
let tSaved = false;

function tCardHTML(t, saved) {
  const collocations = (t.collocations || [])
    .filter((c) => c.en)
    .map((c) => `<li><b>${esc(c.en)}</b><span>${esc(c.zh)}</span></li>`)
    .join('');

  const example =
    t.example && t.example.en
      ? `<p class="eg">${esc(t.example.en)}<span>${esc(t.example.zh)}</span></p>`
      : '<p class="eg" style="color:var(--text-4);font-style:normal">（这次没给例句）</p>';

  return `
    <div class="tcard fade-in">
      <div class="tcard-head">
        <h2>${esc(t.word)}</h2>
        ${t.phonetic ? `<span class="tphon">${esc(t.phonetic)}</span>` : ''}
        <button class="icon-btn" id="tSpeak" title="朗读">
          <svg viewBox="0 0 24 24"><path d="M11 5L6 9H3v6h3l5 4V5zM16 9a4 4 0 010 6M19 6a8 8 0 010 12"/></svg>
        </button>
        <button class="btn-add ${saved ? 'done' : ''}" id="tAdd" style="margin-left:auto">
          ${
            saved
              ? '<svg viewBox="0 0 24 24"><path d="M4 12.5l5 5L20 6.5"/></svg><span>已在生词本</span>'
              : '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg><span>加入生词本</span>'
          }
        </button>
      </div>

      ${
        t.pos
          ? `<div class="pos-row"><span class="pos">${esc(t.pos)}</span></div>`
          : '<div style="height:10px"></div>'
      }

      <p class="tdef">${esc(t.definition)}</p>

      <div class="tcols">
        <div class="block">
          <h4>常见搭配</h4>
          <ul class="pairs">${collocations || '<li style="color:var(--text-4);font-size:12px">—</li>'}</ul>
        </div>
        <div class="block">
          <h4>例句</h4>
          ${example}
        </div>
      </div>
    </div>`;
}

async function renderTranslate() {
  main.innerHTML = `
    <div class="vhead">
      <h1>翻译</h1>
      <span class="sub">不依赖划词和全局快捷键，随时都能用</span>
    </div>

    <div class="tbox">
      <textarea id="tinput" spellcheck="false"
        placeholder="输入或粘贴英文，可以是单词、短语或整句…">${esc(tInput)}</textarea>
      <div class="tbar">
        <span class="thint"><kbd>Enter</kbd> 翻译 · <kbd>Shift</kbd> + <kbd>Enter</kbd> 换行</span>
        <button class="btn-primary" id="tgo" style="margin-left:auto" ${tLoading ? 'disabled' : ''}>
          ${tLoading ? '<span class="spinner" style="border-top-color:#1a1206"></span> 翻译中' : '翻译'}
        </button>
      </div>
    </div>

    <div id="tresult">
      ${tError ? `<div class="test-result bad" style="display:block">${esc(tError)}</div>` : ''}
      ${tResult ? tCardHTML(tResult, tSaved) : ''}
    </div>
  `;

  const ta = document.getElementById('tinput');
  if (ta) {
    ta.addEventListener('input', () => {
      tInput = ta.value;
    });
    ta.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        runTranslatePage();
      }
    });
    // 只有从别的页面切进来才抢焦点，避免翻译完打断输入
    if (!tResult && !tLoading) {
      ta.focus();
    }
  }

  const go = document.getElementById('tgo');
  if (go) go.onclick = runTranslatePage;

  const add = document.getElementById('tAdd');
  if (add && tResult) {
    add.onclick = async () => {
      if (tSaved) return;
      add.disabled = true;
      try {
        await invoke('add_word', {
          input: {
            word: tResult.word,
            phonetic: tResult.phonetic || '',
            pos: tResult.pos || '',
            definition: tResult.definition || '',
            context: tInput.trim(),
          },
        });
        tSaved = true;
        refreshBadges();
        renderTranslate();
      } catch (e) {
        add.disabled = false;
        tError = String(e);
        renderTranslate();
      }
    };
  }

  const speak = document.getElementById('tSpeak');
  if (speak && tResult) {
    speak.onclick = () => {
      try {
        const u = new SpeechSynthesisUtterance(tResult.word);
        u.lang = 'en-US';
        u.rate = 0.9;
        speechSynthesis.cancel();
        speechSynthesis.speak(u);
      } catch (_) {}
    };
  }
}

async function runTranslatePage() {
  const text = tInput.trim();
  if (!text || tLoading) return;

  tLoading = true;
  tError = '';
  await renderTranslate();

  try {
    const res = await invoke('translate', { text });
    tResult = res.translation;
    tSaved = res.saved;
  } catch (e) {
    tError = String(e);
    tResult = null;
  }
  tLoading = false;
  await renderTranslate();
}

// ---------------- 生词本 ----------------

async function renderVocab() {
  main.innerHTML = `<div class="loading"><span class="spinner"></span>读取生词本…</div>`;

  let words = [];
  let stats = { total: 0, mastered: 0, pending_review: 0 };
  try {
    words = await invoke('list_words', { query: searchTerm || null });
    stats = await invoke('stats');
  } catch (e) {
    main.innerHTML = `<div class="empty"><h3>读不出来</h3><p>${esc(e)}</p></div>`;
    return;
  }

  document.getElementById('navCount').textContent = stats.total;
  document.getElementById('navDue').textContent = stats.pending_review;

  const rows = words
    .map((w) => {
      const dots = Array.from({ length: 5 }, (_, i) => {
        if (i < w.familiarity) return '<i class="on"></i>';
        return '<i></i>';
      }).join('');
      return `
      <div class="wrow" data-id="${esc(w.id)}">
        <div class="wmain">
          <div class="wtop">
            <span class="w">${esc(w.word)}</span>
            ${w.phonetic ? `<span class="ph">${esc(w.phonetic)}</span>` : ''}
            ${w.pos ? `<span class="wpos">${esc(w.pos)}</span>` : ''}
          </div>
          <div class="wdef">${esc(w.definition)}</div>
          ${w.context ? `<div class="wctx">${esc(w.context)}</div>` : ''}
        </div>
        <div class="lvl">${dots}</div>
        <button class="wdel" data-del="${esc(w.id)}" title="删除">
          <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6L6 18"/></svg>
        </button>
      </div>`;
    })
    .join('');

  main.innerHTML = `
    <div class="vhead">
      <h1>生词本</h1>
      <span class="sub">${stats.total} 词 · 已稳固 ${stats.mastered} · 待复习 ${stats.pending_review}</span>
    </div>
    <div class="toolbar">
      <label class="searchbox">
        <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M16.5 16.5L21 21"/></svg>
        <input id="q" type="text" placeholder="搜索单词、释义或原文语境…" value="${esc(searchTerm)}" />
      </label>
      <button class="btn-ghost" id="goReview">去复习</button>
    </div>
    ${
      words.length
        ? `<div class="wlist fade-in">${rows}</div>`
        : `<div class="empty fade-in">
             <div class="ico"><svg viewBox="0 0 24 24"><path d="M4 7h9.5L11 4.5M20 17h-9.5L13 19.5"/></svg></div>
             <h3>${searchTerm ? '没找到匹配的词' : '还没有生词'}</h3>
             <p>${
               searchTerm
                 ? '换个关键词试试，或者清空搜索框。'
                 : '在浏览器、IDE 或任何文档里划选一段英文，点上方浮出的「翻译」，再点卡片右下角的「加入生词表」。'
             }</p>
           </div>`
    }
  `;

  const q = document.getElementById('q');
  let timer = null;
  q.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      searchTerm = q.value.trim();
      renderVocab();
      // 重新渲染后把焦点和光标位置还回搜索框
      const nq = document.getElementById('q');
      if (nq) {
        nq.focus();
        nq.setSelectionRange(nq.value.length, nq.value.length);
      }
    }, 220);
  });

  document.getElementById('goReview').onclick = () => {
    view = 'review';
    nav.querySelectorAll('button').forEach((x) =>
      x.classList.toggle('on', x.dataset.view === 'review')
    );
    render();
  };

  main.querySelectorAll('[data-del]').forEach((btn) => {
    btn.onclick = async (e) => {
      e.stopPropagation();
      const id = btn.dataset.del;
      try {
        await invoke('delete_word', { id });
        renderVocab();
      } catch (err) {
        console.error(err);
      }
    };
  });
}

// ---------------- 复习 ----------------

async function renderReview() {
  main.innerHTML = `<div class="loading"><span class="spinner"></span>准备中…</div>`;

  let items = [];
  try {
    items = await invoke('get_review');
  } catch (e) {
    main.innerHTML = `<div class="empty"><h3>读不出来</h3><p>${esc(e)}</p></div>`;
    return;
  }

  if (items.length === 0) {
    return renderReviewIdle();
  }

  // 有新题就把会话重置到第一题
  if (review.items.length === 0 || review.items[0].id !== items[0].id) {
    review.items = items;
    review.idx = 0;
    review.answered = false;
    review.chosen = -1;
    review.result = null;
    review.results = [];
  }

  drawQuestion();
}

function renderReviewIdle() {
  main.innerHTML = `
    <div class="review-wrap fade-in">
      <div class="vhead"><h1>复习</h1><span class="sub">AI 按你的生词表现出题</span></div>
      <div class="empty" style="padding:48px 20px">
        <div class="ico"><svg viewBox="0 0 24 24"><path d="M12 3l9 16H3l9-16zM12 10v4M12 17h.01"/></svg></div>
        <h3>还没有待复习的题</h3>
        <p>点下面的按钮，AI 会从生词本里挑词，<b>现场造一个它不是原句的句子</b>，让你判断词在句中的意思。</p>
        <div style="margin-top:22px;display:flex;gap:10px;justify-content:center">
          <button class="btn-primary" id="btnGen" style="margin:0">生成今日复习</button>
        </div>
        <div class="test-result" id="genResult"></div>
      </div>
    </div>`;

  const btn = document.getElementById('btnGen');
  btn.onclick = async () => {
    btn.disabled = true;
    btn.innerHTML = '<span class="spinner" style="border-top-color:#1a1206"></span> 生成中…';
    try {
      const n = await invoke('prepare_review');
      const box = document.getElementById('genResult');
      if (n === 0) {
        box.className = 'test-result bad';
        box.textContent = '没有可出题的词。先去划几个词存进生词本，或者今天已经出满了。';
        btn.disabled = false;
        btn.textContent = '生成今日复习';
      } else {
        await renderReview();
      }
    } catch (e) {
      const box = document.getElementById('genResult');
      box.className = 'test-result bad';
      box.textContent = String(e);
      btn.disabled = false;
      btn.textContent = '生成今日复习';
    }
  };
}

function drawQuestion() {
  const it = review.items[review.idx];
  if (!it) {
    return renderReviewDone();
  }

  const total = review.items.length;
  const pct = total ? Math.round((review.idx / total) * 100) : 0;

  const dots = review.items
    .map((_, i) => {
      if (i === review.idx) return '<i class="now"></i>';
      const r = review.results[i];
      if (r === true) return '<i class="ok"></i>';
      if (r === false) return '<i class="bad"></i>';
      return '<i></i>';
    })
    .join('');

  const correctCount = review.results.filter((r) => r === true).length;
  const answeredCount = review.results.filter((r) => r !== undefined).length;

  const opts = it.options
    .map((text, i) => {
      let cls = '';
      if (review.answered) {
        if (i === review.result?.answer_index) cls = 'right';
        else if (i === review.chosen) cls = 'wrong';
      }
      const letter = String.fromCharCode(65 + i);
      return `<button class="opt ${cls}" data-opt="${i}" ${review.answered ? 'disabled' : ''}>
                <span class="k">${letter}</span>${esc(text)}
              </button>`;
    })
    .join('');

  const explanation = review.answered
    ? `<div class="explain fade-in">
         <div class="eh">
           <svg viewBox="0 0 24 24"><path d="M12 3l9 16H3l9-16zM12 10v4M12 17h.01"/></svg>
           <span>${review.result?.correct ? '答对了' : 'AI 解析'}</span>
           <em>${review.result?.correct ? '' : `你选了 ${String.fromCharCode(65 + review.chosen)}`}</em>
         </div>
         <p>${esc(it.explanation || '（这题模型没给解析）')}</p>
       </div>`
    : '';

  main.innerHTML = `
    <div class="review-wrap fade-in">
      <div class="rprog"><i style="width:${pct}%"></i></div>
      <div class="rhead">
        <span class="num">第 <b>${review.idx + 1}</b> / ${total} 题</span>
        <span class="acc">正确 ${correctCount} / ${answeredCount}</span>
      </div>
      <p class="ask">选出划线词在这个句子里的意思</p>
      <div class="sentence">
        ${hl(it.sentence)}
        ${it.sentence_zh ? `<span class="zh">${esc(it.sentence_zh)}</span>` : ''}
      </div>
      <div class="opts" id="opts">${opts}</div>
      ${explanation}
      <div class="rfoot">
        <div class="dots-prog">${dots}</div>
        <button class="btn-primary" id="btnNext" ${review.answered ? '' : 'disabled'}>
          ${review.idx + 1 === total ? '看结果' : '下一题'}
          <svg viewBox="0 0 24 24"><path d="M5 12h13M13 7l5 5-5 5"/></svg>
        </button>
      </div>
    </div>`;

  main.querySelectorAll('[data-opt]').forEach((b) => {
    b.onclick = () => submitAnswer(Number(b.dataset.opt), b);
  });

  document.getElementById('btnNext').onclick = () => {
    review.idx += 1;
    review.answered = false;
    review.chosen = -1;
    review.result = null;
    drawQuestion();
  };
}

async function submitAnswer(i, btnEl) {
  if (review.answered) return;
  const it = review.items[review.idx];
  review.chosen = i;
  review.answered = true;

  main.querySelectorAll('[data-opt]').forEach((b) => (b.disabled = true));
  btnEl.style.opacity = '0.7';

  try {
    const res = await invoke('answer_question', { id: it.id, chosen: i });
    review.result = res;
    review.results[review.idx] = res.correct;
  } catch (e) {
    review.result = { correct: false, answer_index: it.answer_index, explanation: String(e) };
    review.results[review.idx] = false;
  }
  drawQuestion();
}

function renderReviewDone() {
  const total = review.items.length;
  const right = review.results.filter((r) => r === true).length;
  const rate = total ? Math.round((right / total) * 100) : 0;

  main.innerHTML = `
    <div class="review-wrap fade-in">
      <div class="vhead"><h1>这轮结束了</h1></div>
      <div class="empty" style="padding:40px 20px">
        <div class="ico" style="background:rgba(94,224,176,.12)">
          <svg viewBox="0 0 24 24" style="stroke:var(--mint)"><path d="M4 12.5l5 5L20 6.5"/></svg>
        </div>
        <h3>${right} / ${total} 答对（${rate}%）</h3>
        <p>答对的词熟悉度加一，答错的减一，下轮会更早遇到它们。</p>
        <div style="margin-top:22px;display:flex;gap:10px;justify-content:center">
          <button class="btn-primary" id="btnAgain" style="margin:0">再来一轮</button>
          <button class="btn-ghost" id="btnBackVocab">回生词本</button>
        </div>
      </div>
    </div>`;

  document.getElementById('btnAgain').onclick = async () => {
    review.items = [];
    review.results = [];
    review.idx = 0;
    await renderReview();
  };
  document.getElementById('btnBackVocab').onclick = () => {
    view = 'vocab';
    nav.querySelectorAll('button').forEach((x) =>
      x.classList.toggle('on', x.dataset.view === 'vocab')
    );
    render();
  };
  refreshBadges();
}

// ---------------- 设置 ----------------

async function renderSettings() {
  main.innerHTML = `<div class="loading"><span class="spinner"></span>读取设置…</div>`;

  let s;
  try {
    s = await invoke('get_settings');
  } catch (e) {
    main.innerHTML = `<div class="empty"><h3>读不出来</h3><p>${esc(e)}</p></div>`;
    return;
  }
  settingsCache = s;

  main.innerHTML = `
    <div class="settings fade-in">
      <div class="vhead"><h1>设置</h1><span class="sub">改完记得保存</span></div>

      <div class="sgroup">
        <div class="gh">翻译引擎</div>
        <div class="srow">
          <div class="lb">用哪个跑翻译</div>
          <div class="seg" id="segEngine">
            <button data-engine="cloud" class="${s.engine === 'cloud' ? 'on' : ''}">云端 API</button>
            <button data-engine="ollama" class="${s.engine === 'ollama' ? 'on' : ''}">本地 Ollama</button>
          </div>
        </div>

        <div id="cloudBox" style="${s.engine === 'cloud' ? '' : 'display:none'}">
          <div class="srow">
            <div class="lb">接口地址<i>OpenAI 兼容，末尾一般带 /v1</i></div>
            <input type="text" id="cloudBase" value="${esc(s.cloud_base_url)}" />
          </div>
          <div class="srow">
            <div class="lb">API Key</div>
            <input type="password" id="cloudKey" value="${esc(s.cloud_api_key)}" placeholder="sk-..." />
          </div>
          <div class="srow">
            <div class="lb">模型名</div>
            <input type="text" id="cloudModel" value="${esc(s.cloud_model)}" placeholder="deepseek-chat" />
          </div>
        </div>

        <div id="ollamaBox" style="${s.engine === 'ollama' ? '' : 'display:none'}">
          <div class="srow">
            <div class="lb">服务地址<i>本机默认 127.0.0.1:11434</i></div>
            <input type="text" id="ollamaBase" value="${esc(s.ollama_base_url)}" />
          </div>
          <div class="srow">
            <div class="lb">模型名<i>得先 ollama pull 过</i></div>
            <input type="text" id="ollamaModel" value="${esc(s.ollama_model)}" placeholder="qwen2.5:7b" />
          </div>
        </div>

        <div class="srow">
          <button class="btn-ghost" id="btnTest">测试连接</button>
        </div>
        <div class="test-result" id="testResult"></div>
      </div>

      <div class="sgroup">
        <div class="gh">取词</div>

        <div class="srow">
          <div class="lb">划选后浮出翻译按钮<i>关掉后划词完全安静，不弹任何东西</i></div>
          <div class="switch ${s.auto_popup ? 'on' : ''}" id="swAuto"></div>
        </div>

        <div class="srow">
          <div class="lb">启用快捷键<i>关掉后组合键失效，配置留着不丢</i></div>
          <div class="switch ${s.hotkey_enabled ? 'on' : ''}" id="swHotkey"></div>
        </div>

        <div class="srow">
          <div class="lb">快捷键组合<i>点输入框后直接按；Backspace 清空</i></div>
          <input type="text" id="hotkey" class="keycap ${s.hotkey_enabled ? '' : 'off'}" readonly value="${esc(s.hotkey || '未设置')}" />
        </div>

        <p class="hint">
          两个都关掉，划译就只在托盘里待着，不会主动打扰你。
        </p>
        <p class="hint">
          取词靠模拟 <code>Ctrl</code> + <code>C</code> 读剪贴板并还原原内容。快捷键是全局低级钩子，
          按键会同时传给当前应用——别设成 <code>Ctrl</code> + <code>C</code> 这类应用自己要用的组合。
        </p>
      </div>

      <div class="sgroup">
        <div class="gh">复习</div>
        <div class="srow">
          <div class="lb">每轮题量<i>一次生成后缓存起来，不重复花 token</i></div>
          <input type="number" id="dailyLimit" min="3" max="30" value="${s.daily_limit}" />
        </div>
      </div>

      <div class="save-bar">
        <button class="btn-primary" id="btnSave" style="margin-left:0">保存设置</button>
        <span class="tip" id="saveTip">已保存</span>
      </div>
    </div>`;

  // 引擎切换
  const seg = document.getElementById('segEngine');
  seg.addEventListener('click', (e) => {
    const b = e.target.closest('button[data-engine]');
    if (!b) return;
    seg.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b));
    const engine = b.dataset.engine;
    document.getElementById('cloudBox').style.display = engine === 'cloud' ? '' : 'none';
    document.getElementById('ollamaBox').style.display = engine === 'ollama' ? '' : 'none';
  });

  const swAuto = document.getElementById('swAuto');
  swAuto.onclick = () => swAuto.classList.toggle('on');

  // ---- 快捷键录制 ----
  const hkInput = document.getElementById('hotkey');
  const swHotkey = document.getElementById('swHotkey');
  hotkeyValue = s.hotkey || '';

  // 总开关关掉时录制框变灰且不可编辑，但值保留
  swHotkey.onclick = () => {
    const on = swHotkey.classList.toggle('on');
    hkInput.classList.toggle('off', !on);
    if (!on) hkInput.blur();
  };

  hkInput.addEventListener('keydown', (e) => {
    if (hkInput.classList.contains('off')) return;
    e.preventDefault();
    e.stopPropagation();

    if (e.key === 'Backspace' || e.key === 'Delete') {
      hotkeyValue = '';
      hkInput.value = '未设置';
      return;
    }

    const mods = [];
    if (e.ctrlKey) mods.push('Ctrl');
    if (e.altKey) mods.push('Alt');
    if (e.shiftKey) mods.push('Shift');
    if (e.metaKey) mods.push('Meta');

    // 只按了修饰键，给个「还在录」的提示
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
      hkInput.value = mods.length ? mods.join('+') + ' + …' : '…';
      return;
    }

    const name = keyNameOf(e.key);
    if (!name) {
      hkInput.value = '这个键不支持';
      return;
    }
    if (!mods.length) {
      hkInput.value = '至少加一个 Ctrl / Alt / Shift';
      return;
    }

    hotkeyValue = [...mods, name].join('+');
    hkInput.value = hotkeyValue;
  });

  hkInput.addEventListener('blur', () => {
    hkInput.value = hotkeyValue || '未设置';
  });

  document.getElementById('btnTest').onclick = async () => {
    const box = document.getElementById('testResult');
    box.className = 'test-result';
    box.textContent = '测试中…';
    box.classList.add('ok');
    try {
      await doSave(true);
      const msg = await invoke('test_engine');
      box.className = 'test-result ok';
      box.textContent = '通了：' + msg;
    } catch (e) {
      box.className = 'test-result bad';
      box.textContent = String(e);
    }
  };

  document.getElementById('btnSave').onclick = () => doSave(false);
}

async function doSave(silent) {
  const engine = document.querySelector('#segEngine button.on')?.dataset.engine || 'cloud';
  const val = (id) => document.getElementById(id)?.value ?? '';

  const s = {
    engine,
    cloud_base_url: val('cloudBase') || 'https://api.deepseek.com/v1',
    cloud_api_key: val('cloudKey'),
    cloud_model: val('cloudModel') || 'deepseek-chat',
    ollama_base_url: val('ollamaBase') || 'http://127.0.0.1:11434',
    ollama_model: val('ollamaModel') || 'qwen2.5:7b',
    auto_popup: document.getElementById('swAuto').classList.contains('on'),
    daily_limit: Math.min(30, Math.max(3, Number(val('dailyLimit')) || 8)),
    drag_threshold: 12,
    hotkey: hotkeyValue,
    hotkey_enabled: document.getElementById('swHotkey').classList.contains('on'),
  };

  try {
    await invoke('save_settings', { settings: s });
    refreshBadges();
    if (!silent) {
      const tip = document.getElementById('saveTip');
      tip.classList.add('show');
      setTimeout(() => tip.classList.remove('show'), 1600);
    }
  } catch (e) {
    alert('保存失败：' + e);
  }
}

// ---------------- 启动 ----------------

// 浮层里点「去设置」会推这个事件过来
window.__TAURI__.event.listen('goto', (e) => {
  const v = e.payload;
  if (!v || !document.querySelector(`#nav button[data-view="${v}"]`)) return;
  view = v;
  nav.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x.dataset.view === v));
  render();
});

refreshBadges();
render();
