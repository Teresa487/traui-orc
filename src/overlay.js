import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const shotEl = document.getElementById("shot");
const selEl = document.getElementById("sel");
const sizeEl = document.getElementById("size");
const hintEl = document.getElementById("hint");
const loadingEl = document.getElementById("loading");

const win = getCurrentWindow();

// 当前显示器信息，含 DPI 缩放
let monitor = { index: 0, scale_factor: 1, width: 0, height: 0 };

// 图片原始像素尺寸
let imgW = 0;
let imgH = 0;

let dragging = false;
let startX = 0;
let startY = 0;
let curX = 0;
let curY = 0;

/** 把 CSS 像素换算成截图里的物理像素 */
function toPhysical(cssPx) {
  return Math.round(cssPx * monitor.scale_factor);
}

/** 找到遮罩窗口所在的显示器 */
async function resolveMonitor() {
  const monitors = await invoke("list_monitors");
  const pos = await win.outerPosition();
  const size = await win.outerSize();
  const cx = pos.x + size.width / 2;
  const cy = pos.y + size.height / 2;

  for (let i = 0; i < monitors.length; i++) {
    const m = monitors[i];
    const right = m.x + m.width / m.scale_factor;
    const bottom = m.y + m.height / m.scale_factor;
    if (cx >= m.x && cx < right && cy >= m.y && cy < bottom) {
      return { ...m, index: i };
    }
  }
  return monitors[0] ? { ...monitors[0], index: 0 } : null;
}

/** 截当前屏幕，铺成背景 */
async function grabScreen() {
  const m = await resolveMonitor();
  if (!m) {
    loadingEl.textContent = "没有检测到显示器";
    return;
  }
  monitor = m;

  const b64 = await invoke("grab_monitor", { index: m.index });
  shotEl.src = `data:image/png;base64,${b64}`;
  await new Promise((r) => {
    if (shotEl.complete) return r();
    shotEl.onload = r;
    shotEl.onerror = r;
  });

  imgW = shotEl.naturalWidth || 1;
  imgH = shotEl.naturalHeight || 1;

  // 缩放系数以实际图片与 CSS 尺寸的比值为准，比系统 DPI 更可靠
  monitor.scale_factor = imgW / Math.max(1, window.innerWidth);

  loadingEl.style.display = "none";
}

function rectFromPoints(x1, y1, x2, y2) {
  return {
    left: Math.min(x1, x2),
    top: Math.min(y1, y2),
    width: Math.abs(x2 - x1),
    height: Math.abs(y2 - y1),
  };
}

function drawSelection() {
  if (!dragging) return;
  const r = rectFromPoints(startX, startY, curX, curY);
  selEl.style.display = "block";
  selEl.style.left = `${r.left}px`;
  selEl.style.top = `${r.top}px`;
  selEl.style.width = `${r.width}px`;
  selEl.style.height = `${r.height}px`;

  sizeEl.style.display = "block";
  const pw = toPhysical(r.width);
  const ph = toPhysical(r.height);
  sizeEl.textContent = `${pw} × ${ph}`;

  // 标签跟在选框右下，接近屏幕边缘时翻到内侧
  let lx = r.left + r.width + 8;
  let ly = r.top + r.height + 8;
  if (lx + 90 > window.innerWidth) lx = r.left - 90;
  if (ly + 22 > window.innerHeight) ly = r.top - 26;
  sizeEl.style.left = `${Math.max(0, lx)}px`;
  sizeEl.style.top = `${Math.max(0, ly)}px`;

  hintEl.style.display = "none";
}

function resetSelection() {
  dragging = false;
  selEl.style.display = "none";
  sizeEl.style.display = "none";
  hintEl.style.display = "block";
}

async function finishSelection() {
  const r = rectFromPoints(startX, startY, curX, curY);
  const wPhys = toPhysical(r.width);
  const hPhys = toPhysical(r.height);

  // 太小多半是误触，忽略
  if (wPhys < 6 || hPhys < 6) {
    resetSelection();
    return;
  }

  loadingEl.style.display = "flex";
  loadingEl.textContent = "识别中……";

  try {
    // 先恢复主窗口，让用户看到结果
    await invoke("end_capture");
    await win.close();

    await invoke("recognize_region", {
      monitorIndex: monitor.index,
      x: toPhysical(r.left),
      y: toPhysical(r.top),
      width: wPhys,
      height: hPhys,
    });
  } catch (e) {
    console.error("识别失败", e);
  }
}

async function cancel() {
  await invoke("end_capture");
  await win.close();
}

/* ---------- 鼠标事件 ---------- */

window.addEventListener("mousedown", (e) => {
  if (e.button === 2) {
    // 右键重选
    resetSelection();
    return;
  }
  if (e.button !== 0) return;
  // 屏蔽默认的窗口拖动
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
  drawSelection();
});

window.addEventListener("mouseup", async (e) => {
  if (e.button !== 0 || !dragging) return;
  curX = e.clientX;
  curY = e.clientY;
  drawSelection();
  dragging = false;
  await finishSelection();
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") cancel();
});

window.addEventListener("contextmenu", (e) => e.preventDefault());

/* ---------- 启动 ---------- */

// 主进程建好窗口后会发 begin-capture，收到再截屏，
// 这样截到的画面里不会有遮罩自己
listen("begin-capture", async () => {
  try {
    await grabScreen();
  } catch (e) {
    loadingEl.textContent = `截屏失败: ${e}`;
  }
});

// 兜底：万一事件在监听注册前就发了
setTimeout(async () => {
  if (loadingEl.style.display !== "none") {
    try {
      await grabScreen();
    } catch (e) {
      loadingEl.textContent = `截屏失败: ${e}`;
    }
  }
}, 700);
