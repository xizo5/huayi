# 划译 Huayi

在任何窗口划选英文 → 上方浮出翻译按钮 → 点开看释义 → 一键进生词本 → AI 拿你的词出题考你。

Tauri 2 + 原生前端（零 npm 依赖）。Windows。

## 跑起来

```bash
cargo run --manifest-path src-tauri/Cargo.toml
```

首次编译约 3 分钟，之后增量 20 秒。

改前端不用重新编译：前端是静态资源，但 Tauri 会把它嵌进二进制，所以**改了 src/ 下的文件需要重新 `cargo build`**。

## 第一次用

1. 启动后主窗口弹出，左侧「设置」
2. 选引擎：
   - **云端 API**（推荐）：填 OpenAI 兼容地址 + Key + 模型名，默认 DeepSeek
   - **本地 Ollama**：确认 `ollama serve` 在跑，模型名要对得上 `ollama list`
3. 点「测试连接」确认通了
4. 去任意窗口划一段英文，浮层会出现

## 交互

| 操作 | 结果 |
|---|---|
| 拖选文字 | 选区上方浮出小胶囊「翻译」 |
| 点胶囊 | 下方展开释义卡片（骨架屏 → 内容） |
| 卡片右下角 | 加入生词本，卡片内出现绿条确认 |
| 点浮层外任意位置 | 浮层收起 |
| 自定义快捷键（默认 `Alt+D`） | 在鼠标当前位置再取一次词。设置页里点输入框直接按组合键即可改 |
| 主窗口点 X | 收进托盘，不退出；托盘菜单里退出 |

快捷键是全局低级钩子，按键会**同时传给当前应用**。所以别设成 `Ctrl+C`、`Ctrl+S` 这类应用自己要用的组合，会打架。

## 已知限制（重要）

**本地模型在这台机器上不可用。** 实测 Ryzen 7 5700G（无独显）：

- `qwen2.5:7b` → 42.5 秒
- `phi3:mini` → 51.1 秒

划词翻译的等待上限大概是 3 秒，所以本地引擎只适合当离线兜底，日常务必用云端 API。想在本地跑得换 `qwen2.5:1.5b` 这类小模型，但释义质量会掉。

**取词靠模拟 `Ctrl+C`**，副作用：

- 读完会还原剪贴板**文本**；如果原本是图片/富文本，会被替换成文本（已知，未处理）
- 在禁止复制的控件里静默失败，不弹窗
- 极少数情况下会和你自己的复制操作抢时序

**UIA 加速通道还没实现**（`selection.rs` 里的 `try_uia()` 是空壳）。加它是为了在 Chrome/VS Code 里免碰剪贴板，需要引入 `uiautomation` crate，会让首次编译多几分钟。

## 目录

```
src/                      前端（静态，无构建）
  index.html              主窗口：生词本 / 复习 / 设置
  popup.html              浮层：按钮态 + 卡片态
  main.js  popup.js
  styles/

src-tauri/src/
  lib.rs                  启动、托盘、窗口事件
  selection.rs            全局钩子 + 剪贴板取词
  popup.rs                浮层定位、尺寸、免激活显示
  commands.rs             前端能调的 13 个命令
  translate/              引擎抽象：cloud / ollama + 两个 prompt
  db.rs                   SQLite（words / questions / kv）
  models.rs

prototype.html            最早的产品原型（设计基准，可单独打开看）
```

## 数据

`%APPDATA%\com.huayi.desktop\huayi.db`

三张表：`words` 生词、`questions` 预生成的复习题、`kv` 设置。

复习题是**批量预生成后缓存**的：点一次「生成今日复习」调一次 AI 出满 N 道，之后答题不再花 token。

## 调试

```bash
# 启动后直接摆一个浮层出来，省得去划词
HUAYI_DEBUG_POPUP=1 cargo run --manifest-path src-tauri/Cargo.toml
```

前端调样式可以脱离 Tauri，直接用浏览器打开 `src/popup.html`（自带 mock 数据）。加 `?view=btn` 看按钮态。
