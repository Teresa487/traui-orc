import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-opener";

const $ = (id) => document.getElementById(id);

const els = {
  status: $("status"),
  result: $("result"),
  hotkeyHint: $("hotkey-hint"),
  btnCapture: $("btn-capture"),
  btnFile: $("btn-file"),
  btnSettings: $("btn-settings"),
  btnCopy: $("btn-copy"),
  btnClear: $("btn-clear"),
  btnTest: $("btn-test"),
  btnSave: $("btn-save-settings"),
  btnCloseSettings: $("btn-close-settings"),
  modal: $("settings-modal"),
  testResult: $("test-result"),
  fileInput: $("file-input"),
  cfg: {
    apiBase: $("cfg-api-base"),
    apiKey: $("cfg-api-key"),
    model: $("cfg-model"),
    hotkey: $("cfg-hotkey"),
    mode: $("cfg-mode"),
    stream: $("cfg-stream"),
    autoCopy: $("cfg-autocopy"),
    upscale: $("cfg-upscale"),
  },
};

let config = null;

function setStatus(text, kind = "") {
  els.status.textContent = text;
  els.status.className = kind;
}

/* ---------- 配置 ---------- */

async function loadConfig() {
  config = await invoke("get_config");
  fillForm();
}

function fillForm() {
  els.cfg.apiBase.value = config.api_base;
  els.cfg.apiKey.value = config.api_key;
  els.cfg.model.value = config.model;
  els.cfg.hotkey.value = config.hotkey;
  els.cfg.stream.checked = config.stream;
  els.cfg.autoCopy.checked = config.auto_copy;
  els.cfg.upscale.value = config.upscale;

  els.cfg.mode.innerHTML = "";
  for (const m of config.modes) {
    const opt = document.createElement("option");
    opt.value = m.mode;
    opt.textContent = m.mode;
    if (m.mode === config.active_mode) opt.selected = true;
    els.cfg.mode.appendChild(opt);
  }

  els.hotkeyHint.textContent = config.hotkey;
}

function readForm() {
  return {
    ...config,
    api_base: els.cfg.apiBase.value.trim(),
    api_key: els.cfg.apiKey.value.trim(),
    model: els.cfg.model.value.trim(),
    hotkey: els.cfg.hotkey.value.trim(),
    active_mode: els.cfg.mode.value,
    stream: els.cfg.stream.checked,
    auto_copy: els.cfg.autoCopy.checked,
    upscale: Math.max(1, Math.min(4, parseInt(els.cfg.upscale.value, 10) || 1)),
  };
}

/* ---------- 识别结果 ---------- */

function appendResult(text) {
  els.result.value += text;
  els.result.scrollTop = els.result.scrollHeight;
}

function setResult(text) {
  els.result.value = text;
}

async function copyResult() {
  const t = els.result.value;
  if (!t) return;
  await writeText(t);
  setStatus("已复制到剪贴板", "ok");
  setTimeout(() => setStatus("就绪"), 1500);
}

/* ---------- 截图流程 ---------- */

async function startCapture() {
  setStatus("准备截图……", "busy");
  try {
    await invoke("begin_capture");
  } catch (e) {
    setStatus(`启动截图失败: ${e}`, "err");
  }
}

/* 遮罩窗口回传选区后，在这里发起识别 */
async function runRecognize(monitorIndex, x, y, w, h) {
  setStatus("识别中……", "busy");
  setResult("");
  try {
    const text = await invoke("recognize_region", {
      monitorIndex,
      x,
      y,
      width: w,
      height: h,
    });
    if (!config.stream) setResult(text);
    setStatus("识别完成", "ok");
    if (config.auto_copy && els.result.value) {
      await writeText(els.result.value);
      setStatus("识别完成，已复制", "ok");
    }
  } catch (e) {
    setStatus(`识别失败: ${e}`, "err");
  }
}

/* ---------- 文件识别 ---------- */

async function pickFile() {
  els.fileInput.click();
}

els.fileInput.addEventListener("change", async () => {
  const f = els.fileInput.files?.[0];
  if (!f) return;
  setStatus("识别中……", "busy");
  setResult("");

  // 浏览器里拿不到真实路径，只能把内容读到内存再交给后端
  const buf = new Uint8Array(await f.arrayBuffer());
  try {
    const text = await invoke("recognize_image_bytes", {
      data: Array.from(buf),
      filename: f.name,
    });
    setResult(text);
    setStatus("识别完成", "ok");
    if (config.auto_copy && text) {
      await writeText(text);
      setStatus("识别完成，已复制", "ok");
    }
  } catch (e) {
    setStatus(`识别失败: ${e}`, "err");
  }
  els.fileInput.value = "";
});

/* ---------- 事件绑定 ---------- */

els.btnCapture.addEventListener("click", startCapture);
els.btnFile.addEventListener("click", pickFile);
els.btnCopy.addEventListener("click", copyResult);
els.btnClear.addEventListener("click", () => {
  setResult("");
  setStatus("就绪");
});

els.btnSettings.addEventListener("click", () => {
  els.modal.classList.remove("hidden");
  els.testResult.textContent = "";
  els.testResult.className = "test-result";
});

els.btnCloseSettings.addEventListener("click", () => {
  els.modal.classList.add("hidden");
});

els.btnSave.addEventListener("click", async () => {
  const next = readForm();
  try {
    await invoke("save_config", { cfg: next });
    config = next;
    fillForm();
    els.modal.classList.add("hidden");
    setStatus("设置已保存（快捷键需重启生效）", "ok");
  } catch (e) {
    setStatus(`保存失败: ${e}`, "err");
  }
});

els.btnTest.addEventListener("click", async () => {
  const c = readForm();
  els.testResult.textContent = "测试中……";
  els.testResult.className = "test-result";
  try {
    const msg = await invoke("test_connection", {
      apiBase: c.api_base,
      apiKey: c.api_key,
      model: c.model,
    });
    els.testResult.textContent = msg;
    els.testResult.className = "test-result ok";
  } catch (e) {
    els.testResult.textContent = String(e);
    els.testResult.className = "test-result err";
  }
});

/* 主进程通过全局热键拉起遮罩时，前端同步一下状态 */
listen("begin-capture", () => {
  setStatus("遮罩已打开，请框选区域", "busy");
});

/* 流式输出：后端每收到一段就推过来 */
listen("ocr-chunk", (e) => {
  appendResult(e.payload);
});

/* 快捷键：Ctrl+Enter 复制，Esc 关设置 */
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !els.modal.classList.contains("hidden")) {
    els.modal.classList.add("hidden");
  }
  if (e.ctrlKey && e.key === "Enter") {
    e.preventDefault();
    copyResult();
  }
});

/* ---------- 启动 ---------- */

(async () => {
  await loadConfig();
  if (!config.api_key) {
    setStatus("请先点「设置」填入 API Key", "err");
    els.modal.classList.remove("hidden");
  } else {
    setStatus("就绪，" + config.hotkey + " 开始截图");
  }
})();
