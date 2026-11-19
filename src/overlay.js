import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();

const shadeTop = document.getElementById("shade-top");
const shadeBottom = document.getElementById("shade-bottom");
const shadeLeft = document.getElementById("shade-left");
const shadeRight = document.getElementById("shade-right");
const selEl = document.getElementById("sel");
const sizeEl = document.getElementById("size");
const hintEl = document.getElementById("hint");
const shotEl = document.getElementById("shot");

/** 当前显示器的缩放比：CSS 像素 → 截图物理像素 */
let scaleFactor = 1;

let dragging = false;
let startX = 0;
let startY = 0;
let curX = 0;
let curY = 0;

function resetShades() {
  shadeTop.style.cssText = "left:0;top:0;right:0;height:0";
  shadeBottom.style.cssText = "left:0;right:0;bottom:0;height:0";
  shadeLeft.style.cssText = "left:0;top:0;bottom:0;width:0";
  shadeRight.style.cssText = "right:0;top:0;bottom:0;width:0";
}

/** 用四块遮罩围出选区，中间那块保持透明，看到的就是真实桌面 */
function layoutShades(x1, y1, x2, y2) {
  const left = Math.min(x1, x2);
  const top = Math.min(y1, y2);
  const right = Math.max(x1, x2);
  const bottom = Math.max(y1, y2);
  const w = window.innerWidth;
  const h = window.innerHeight;

  shadeTop.style.cssText = `left:0;top:0;right:0;height:${top}px`;
  shadeBottom.style.cssText = `left:0;top:${bottom}px;right:0;bottom:0`;
  shadeLeft.style.cssText = `left:0;top:${top}px;width:${left}px;height:${bottom - top}px`;
  shadeRight.style.cssText = `left:${right}px;top:${top}px;right:0;height:${bottom - top}px`;

  selEl.style.display = "block";
  selEl.style.left = `${left}px`;
  selEl.style.top = `${top}px`;
  selEl.style.width = `${right - left}px`;
  selEl.style.height = `${bottom - top}px`;

  sizeEl.style.display = "block";
  const pw = Math.round((right - left) * scaleFactor);
  const ph = Math.round((bottom - top) * scaleFactor);
  sizeEl.textContent = `${pw} × ${ph}`;

  // 标签跟着光标右下，靠近边缘时翻到内侧
  let lx = right + 10;
  let ly = bottom + 10;
  if (lx + 80 > w) lx = right - 80;
  if (ly + 20 > h) ly = bottom - 26;
  sizeEl.style.left = `${Math.max(0, lx)}px`;
  sizeEl.style.top = `${Math.max(0, ly)}px`;
}

function resetSelection() {
  dragging = false;
  selEl.style.display = "none";
  sizeEl.style.display = "none";
  hintEl.style.display = "block";
  resetShades();
}

async function finishSelection() {
  const left = Math.min(startX, curX);
  const top = Math.min(startY, curY);
  const w = Math.abs(curX - startX);
  const h = Math.abs(curY - startY);

  const pw = Math.round(w * scaleFactor);
  const ph = Math.round(h * scaleFactor);

  // 太小当成误触
  if (pw < 6 || ph < 6) {
    resetSelection();
    return;
  }

  try {
    await invoke("submit_selection", {
      x: Math.round(left * scaleFactor),
      y: Math.round(top * scaleFactor),
      width: pw,
      height: ph,
    });
    await win.hide();
  } catch (e) {
    hintEl.innerHTML = `<span class="err">提交失败：${e}</span><br />按 Esc 关闭`;
  }
}

async function cancel() {
  await invoke("end_capture");
  await win.hide();
}

/* ---------- 鼠标事件 ---------- */

window.addEventListener("mousedown", (e) => {
  // 右键直接取消
  if (e.button === 2) {
    cancel();
    return;
  }
  if (e.button !== 0) return;
  e.preventDefault();
  dragging = true;
  startX = e.clientX;
  startY = e.clientY;
  curX = startX;
  curY = startY;
  hintEl.style.display = "none";
});

window.addEventListener("mousemove", (e) => {
  if (!dragging) return;
  curX = e.clientX;
  curY = e.clientY;
  layoutShades(startX, startY, curX, curY);
});

window.addEventListener("mouseup", async (e) => {
  if (e.button !== 0 || !dragging) return;
  curX = e.clientX;
  curY = e.clientY;
  layoutShades(startX, startY, curX, curY);
  dragging = false;
  await finishSelection();
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") cancel();
});

window.addEventListener("contextmenu", (e) => e.preventDefault());

/* ---------- 启动 ---------- */

// 缩放比由主进程告知；拿不到就按 1 处理，100% 缩放下依然正确
async function initScale() {
  try {
    const s = await win.scaleFactor();
    if (s > 0) scaleFactor = s;
  } catch {
    scaleFactor = 1;
  }
}

/** 取主进程在遮罩出现前抓好的整屏图，铺成背景 */
async function loadShot(silent = false) {
  // 每次显示都当作一次全新的框选
  resetSelection();
  hintEl.innerHTML =
    '拖拽鼠标框选要识别的区域<br /><kbd>Esc</kbd> 或 <kbd>右键</kbd> 取消';
  try {
    const [, b64] = await invoke("take_overlay_shot");
    shotEl.src = `data:image/png;base64,${b64}`;
    await new Promise((r) => {
      if (shotEl.complete && shotEl.naturalWidth) return r();
      shotEl.onload = r;
      shotEl.onerror = r;
    });
    // 用图片宽度和窗口宽度之比推算缩放，比系统 DPI 更贴合实际渲染
    if (shotEl.naturalWidth && window.innerWidth) {
      scaleFactor = shotEl.naturalWidth / window.innerWidth;
    }
  } catch (e) {
    if (!silent) {
      hintEl.innerHTML = `<span class="err">获取截图失败：${e}</span><br />按 Esc 关闭`;
    }
  }
}

resetShades();

// 主进程每次显示遮罩后都会发这个事件，收到就重新取图。
// 窗口是开机时创建好的，JS 不会随显示重新执行，所以不能只在启动时取一次。
listen("begin-capture", () => {
  loadShot();
});

initScale();

// 兜底：如果页面加载晚于事件，主动试一次。
// 拿不到图会报错，所以静默失败，等事件来了再取。
if (document.readyState === "complete") {
  loadShot(true);
} else {
  window.addEventListener("load", () => loadShot(true));
}
