const TAURI = window.__TAURI__;
const inTauri = !!TAURI;

// 脱离 Tauri 跑（直接用浏览器打开 popup.html 调样式）时用假数据
const mockInvoke = async (cmd) => {
  if (cmd === 'translate') {
    return {
      saved: false,
      translation: {
        word: 'Organize',
        phonetic: '/ˈɔːɡənaɪz/',
        pos: 'v.',
        definition: '整理、组织、安排 —— 把零散的事物归拢成一个有序的整体，或筹划某项活动。',
        collocations: [
          { en: 'organize files', zh: '整理文件' },
          { en: 'organize a meeting', zh: '组织会议' },
        ],
        example: {
          en: 'Please organize text files into a daily information report.',
          zh: '请把文本文件整理成一份每日信息报告。',
        },
      },
    };
  }
  if (cmd === 'stats') return { total: 24, mastered: 16, pending_review: 12 };
  return {};
};

// 包一层再取：直接解构 window.__TAURI__.event.listen 有丢 this 的风险，
// 一旦抛错脚本会中断，后面 btnClose 的绑定就全没了 —— 表现就是浮层关不掉。
const invoke = inTauri ? (cmd, args) => TAURI.core.invoke(cmd, args) : mockInvoke;
const listen = inTauri ? (ev, cb) => TAURI.event.listen(ev, cb) : () => {};

const el = (id) => document.getElementById(id);
const btnView = el('btnView');
const card = el('card');
const content = el('cardContent');
const flash = el('flash');
const dueCount = el('dueCount');

const CARD_W = 400;

let currentText = '';
let current = null; // Translation
let saved = false;

const esc = (s) =>
  String(s ?? '').replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
  );

const nextFrame = () =>
  new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

// ---------------- 尺寸同步 ----------------
// 窗口尺寸 = 内容尺寸，所以每次 DOM 变化都要重新上报
async function syncSize() {
  await nextFrame();
  const h = Math.ceil(card.getBoundingClientRect().height);
  if (!h) return;
  try {
    await invoke('popup_resize', { width: CARD_W, height: h, mode: 'card' });
  } catch (e) {
    console.error('resize failed', e);
  }
}

// ---------------- 三种卡片内容 ----------------

function renderLoading() {
  flash.classList.remove('show');
  content.innerHTML = `
    <div class="sk">
      <div class="bar h"></div>
      <div class="bar"></div>
      <div class="bar s"></div>
      <div class="bar xs"></div>
      <div class="bar"></div>
      <div class="bar s"></div>
    </div>`;
}

function renderError(msg) {
  flash.classList.remove('show');
  content.innerHTML = `
    <div class="err">
      <p class="msg">${esc(msg)}</p>
      <div class="act">
        <button class="btn-ghost" id="btnRetry">重试</button>
        <button class="btn-ghost" id="btnOpenSettings">去设置</button>
      </div>
    </div>`;
  el('btnRetry').onclick = () => runTranslate();
  el('btnOpenSettings').onclick = async () => {
    // 先把主窗口拉起来并切到设置页，再收浮层。
    // 两步分开 catch：上一步失败不该拦住收窗口 —— 否则浮层就关不掉了。
    try {
      await invoke('show_main', { view: 'settings' });
    } catch (e) {
      console.error('show_main failed', e);
    }
    try {
      await invoke('hide_popup');
    } catch (e) {
      console.error('hide_popup failed', e);
    }
  };
}

function renderResult(t, isSaved) {
  // 整句翻译：只显示原文和译文，不出词典字段
  if (t.kind === 'sentence') {
    content.innerHTML = `
      <div class="c-body">
        <p class="src-line">${esc(t.source)}</p>
        <p class="zh-line">${esc(t.sentence_zh)}</p>
      </div>
      <div class="c-foot">
        <div class="foot-left">
          <button class="icon-btn" id="btnSpeakSrc" title="朗读原文">
            <svg viewBox="0 0 24 24"><path d="M11 5L6 9H3v6h3l5 4V5zM16 9a4 4 0 010 6"/></svg>
          </button>
          <button class="icon-btn" id="btnCopyZh" title="复制译文">
            <svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 012-2h8"/></svg>
          </button>
          <button class="icon-btn" id="btnRetranslate" title="重新翻译">
            <svg viewBox="0 0 24 24"><path d="M20 11a8 8 0 10-2.3 5.7M20 5v6h-6"/></svg>
          </button>
        </div>
      </div>`;

    el('btnSpeakSrc').onclick = () => {
      try {
        const u = new SpeechSynthesisUtterance(t.source);
        u.lang = 'en-US';
        u.rate = 0.95;
        speechSynthesis.cancel();
        speechSynthesis.speak(u);
      } catch (_) {}
    };
    el('btnCopyZh').onclick = async () => {
      try {
        await navigator.clipboard.writeText(t.sentence_zh);
        toastCopy('已复制');
      } catch (_) {
        toastCopy('复制失败');
      }
    };
    el('btnRetranslate').onclick = () => runTranslate();
    return;
  }

  const collocations = (t.collocations || [])
    .filter((c) => c.en)
    .map((c) => `<li><b>${esc(c.en)}</b><span>${esc(c.zh)}</span></li>`)
    .join('');

  const example =
    t.example && t.example.en
      ? `<div class="block">
           <h4>例句</h4>
           <p class="eg">${esc(t.example.en)}<span>${esc(t.example.zh)}</span></p>
         </div>`
      : '';

  content.innerHTML = `
    <div class="c-body">
      <div class="word-row">
        <h3 class="word">${esc(t.word)}</h3>
        ${t.phonetic ? `<span class="phon">${esc(t.phonetic)}</span>` : ''}
        <button class="icon-btn" id="btnSpeak" title="朗读">
          <svg viewBox="0 0 24 24"><path d="M11 5L6 9H3v6h3l5 4V5zM16 9a4 4 0 010 6M19 6a8 8 0 010 12"/></svg>
        </button>
      </div>
      ${
        t.pos
          ? `<div class="pos-row"><span class="pos">${esc(t.pos)}</span></div>`
          : '<div style="height:8px"></div>'
      }
      <p class="def">${esc(t.definition)}</p>
      ${
        collocations
          ? `<div class="block"><h4>常见搭配</h4><ul class="pairs">${collocations}</ul></div>`
          : ''
      }
      ${example}
    </div>
    <div class="c-foot">
      <div class="foot-left">
        <button class="icon-btn" id="btnSpeak2" title="朗读">
          <svg viewBox="0 0 24 24"><path d="M11 5L6 9H3v6h3l5 4V5zM16 9a4 4 0 010 6"/></svg>
        </button>
        <button class="icon-btn" id="btnCopy" title="复制释义">
          <svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 012-2h8"/></svg>
        </button>
        <button class="icon-btn" id="btnRetranslate" title="重新翻译">
          <svg viewBox="0 0 24 24"><path d="M20 11a8 8 0 10-2.3 5.7M20 5v6h-6"/></svg>
        </button>
      </div>
      <button class="btn-add ${isSaved ? 'done' : ''}" id="btnAdd">
        ${
          isSaved
            ? '<svg viewBox="0 0 24 24"><path d="M4 12.5l5 5L20 6.5"/></svg><span>已加入生词表</span>'
            : '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg><span>加入生词表</span>'
        }
      </button>
    </div>`;

  const speak = () => {
    try {
      const u = new SpeechSynthesisUtterance(t.word);
      u.lang = 'en-US';
      u.rate = 0.9;
      speechSynthesis.cancel();
      speechSynthesis.speak(u);
    } catch (_) {}
  };
  el('btnSpeak').onclick = speak;
  el('btnSpeak2').onclick = speak;

  el('btnCopy').onclick = async () => {
    const text = `${t.word} ${t.phonetic}\n${t.definition}`;
    try {
      await navigator.clipboard.writeText(text);
      toastCopy('已复制');
    } catch (_) {
      toastCopy('复制失败');
    }
  };

  el('btnRetranslate').onclick = () => runTranslate();

  const btnAdd = el('btnAdd');
  btnAdd.onclick = async () => {
    if (saved) return;
    btnAdd.disabled = true;
    try {
      await invoke('add_word', {
        input: {
          word: t.word,
          phonetic: t.phonetic || '',
          pos: t.pos || '',
          definition: t.definition || '',
          context: currentText,
        },
      });
      saved = true;
      btnAdd.classList.add('done');
      btnAdd.disabled = false;
      btnAdd.innerHTML =
        '<svg viewBox="0 0 24 24"><path d="M4 12.5l5 5L20 6.5"/></svg><span>已加入生词表</span>';
      const s = await invoke('stats').catch(() => null);
      dueCount.textContent = s ? s.pending_review : 0;
      flash.classList.add('show');
      await syncSize();
    } catch (e) {
      btnAdd.disabled = false;
      console.error(e);
    }
  };
}

function toastCopy(msg) {
  const old = content.querySelector('.copy-tip');
  if (old) old.remove();
  const tip = document.createElement('div');
  tip.className = 'copy-tip';
  tip.textContent = msg;
  tip.style.cssText =
    'position:absolute;left:50%;bottom:56px;transform:translateX(-50%);padding:5px 12px;' +
    'border-radius:6px;background:#1c2129;border:1px solid #2e3641;font-size:11px;color:#98a2b1;' +
    'pointer-events:none;';
  card.appendChild(tip);
  setTimeout(() => tip.remove(), 1400);
}

// ---------------- 主流程 ----------------

async function runTranslate() {
  if (!currentText) return;
  btnView.hidden = true;
  card.hidden = false;
  renderLoading();
  await syncSize();

  try {
    const res = await invoke('translate', { text: currentText });
    current = res.translation;
    saved = res.saved;
    renderResult(res.translation, res.saved);
  } catch (e) {
    renderError(String(e));
  }
  await syncSize();
}

function showButton() {
  btnView.hidden = false;
  card.hidden = true;
  flash.classList.remove('show');
  content.innerHTML = '';
}

el('btnTranslate').onclick = runTranslate;
el('btnClose').onclick = () => {
  invoke('hide_popup').catch((e) => console.error('hide_popup failed', e));
};

// 后端取词成功会推这个事件
listen('pick', (event) => {
  const p = event.payload || {};
  currentText = (p.text || '').trim();
  current = null;
  saved = false;
  if (!currentText) return;

  // 快捷键触发：跳过按钮，直接翻。
  // 先把按钮藏掉再渲染骨架，否则窗口已经是卡片尺寸，胶囊会撑满闪一帧。
  if (p.auto) {
    btnView.hidden = true;
    card.hidden = false;
    renderLoading();
    runTranslate();
  } else {
    showButton();
  }
});

// 首次加载时先按按钮态摆正尺寸
syncSize().catch(() => {});

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') invoke('hide_popup');
});

// 浏览器直开调样式：自动跑一遍翻译把卡片摆出来
if (!inTauri) {
  document.body.classList.add('mock');
  const params = new URLSearchParams(location.search);
  if (params.get('view') !== 'btn') {
    currentText = 'Organize text files into a daily information report.';
    setTimeout(() => runTranslate(), 60);
  }
}
