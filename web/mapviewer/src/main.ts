import "./style.css";
import { buildMapPalette } from "./mapColors";

const CHUNK = 32;

type MapMeta = {
  world_name: string;
  chunks_w: number;
  chunks_h: number;
  chunk_size: number;
  cells_width: number;
  cells_height: number;
};

const palette = buildMapPalette();

function apiHeaders(): HeadersInit {
  const h: Record<string, string> = {};
  const t = import.meta.env.VITE_MAPVIEWER_TOKEN as string | undefined;
  if (t) {
    h["Authorization"] = `Bearer ${t}`;
  }
  return h;
}

async function fetchMeta(base: string): Promise<MapMeta> {
  const r = await fetch(`${base}/api/map/meta`, { headers: apiHeaders() });
  if (!r.ok) {
    throw new Error(`/api/map/meta ${r.status}`);
  }
  return r.json() as Promise<MapMeta>;
}

async function fetchChunk(
  base: string,
  cx: number,
  cy: number,
): Promise<Uint8Array> {
  const r = await fetch(`${base}/api/map/chunk/${cx}/${cy}`, {
    headers: apiHeaders(),
  });
  if (!r.ok) {
    throw new Error(`chunk ${cx},${cy}: ${r.status}`);
  }
  const buf = await r.arrayBuffer();
  return new Uint8Array(buf);
}

/** Сырые байты чанка (как на сервере) → пиксели: верх = север (как в Unity `GetCell(..., num4 - j)`). */
function rasterChunkToImageData(raw: Uint8Array): ImageData {
  const img = new ImageData(CHUNK, CHUNK);
  const d = img.data;
  for (let ly = 0; ly < CHUNK; ly++) {
    for (let lx = 0; lx < CHUNK; lx++) {
      const sy = CHUNK - 1 - ly;
      const cell = raw[sy * CHUNK + lx] ?? 0;
      const po = cell * 4;
      const out = (ly * CHUNK + lx) * 4;
      d[out] = palette[po]!;
      d[out + 1] = palette[po + 1]!;
      d[out + 2] = palette[po + 2]!;
      d[out + 3] = palette[po + 3]!;
    }
  }
  return img;
}

async function main() {
  const base = (import.meta.env.VITE_API_BASE as string | undefined) ?? "";
  const metaEl = document.getElementById("meta");
  const canvas = document.getElementById("cv") as HTMLCanvasElement;
  const ctx = canvas.getContext("2d");
  if (!ctx || !metaEl) {
    return;
  }

  let meta: MapMeta;
  try {
    meta = await fetchMeta(base);
  } catch (e) {
    metaEl.textContent = `Ошибка загрузки meta: ${e}`;
    return;
  }

  metaEl.textContent = `${meta.world_name} · ${meta.cells_width}×${meta.cells_height} клеток · чанк ${meta.chunk_size}×${meta.chunk_size}`;

  const chunkCache = new Map<string, ImageBitmap>();

  async function getChunkBitmap(cx: number, cy: number): Promise<ImageBitmap> {
    const key = `${cx},${cy}`;
    const hit = chunkCache.get(key);
    if (hit) {
      return hit;
    }
    const raw = await fetchChunk(base, cx, cy);
    const id = rasterChunkToImageData(raw);
    const bmp = await createImageBitmap(id);
    chunkCache.set(key, bmp);
    return bmp;
  }

  let cx = meta.cells_width / 2;
  let cy = meta.cells_height / 2;
  let zoom = 1;
  let dragging = false;
  let px = 0;
  let py = 0;

  function resize() {
    const r = canvas.getBoundingClientRect();
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.floor(r.width * dpr);
    canvas.height = Math.floor(r.height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  resize();
  window.addEventListener("resize", () => {
    resize();
    scheduleDraw();
  });

  canvas.addEventListener("pointerdown", (e) => {
    dragging = true;
    px = e.clientX;
    py = e.clientY;
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointerup", (e) => {
    dragging = false;
    canvas.releasePointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) {
      return;
    }
    const dx = e.clientX - px;
    const dy = e.clientY - py;
    px = e.clientX;
    py = e.clientY;
    cx -= dx / zoom;
    cy -= dy / zoom;
    scheduleDraw();
  });

  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const factor = e.deltaY > 0 ? 0.9 : 1.1;
      const nz = Math.min(64, Math.max(0.125, zoom * factor));
      zoom = nz;
      scheduleDraw();
    },
    { passive: false },
  );

  /** Не грузить сотни чанков сразу — браузер зависает. */
  const MAX_CHUNKS = 384;

  let raf = 0;
  function scheduleDraw() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => void drawFrame());
  }

  async function drawFrame() {
    try {
      const w = canvas.getBoundingClientRect().width;
      const h = canvas.getBoundingClientRect().height;
      if (w < 2 || h < 2) {
        raf = requestAnimationFrame(() => void drawFrame());
        return;
      }
      ctx.fillStyle = "#1a1a1e";
      ctx.fillRect(0, 0, w, h);

      const halfW = w / (2 * zoom);
      const halfH = h / (2 * zoom);
      const wl = cx - halfW;
      const wt = cy - halfH;
      const wr = wl + w / zoom;
      const wb = wt + h / zoom;
      const eps = 1e-6;

      let c0x = Math.max(0, Math.floor(wl / CHUNK));
      let c0y = Math.max(0, Math.floor(wt / CHUNK));
      let c1x = Math.min(meta.chunks_w - 1, Math.floor((wr - eps) / CHUNK));
      let c1y = Math.min(meta.chunks_h - 1, Math.floor((wb - eps) / CHUNK));
      if (c1x < c0x) {
        [c0x, c1x] = [c1x, c0x];
      }
      if (c1y < c0y) {
        [c0y, c1y] = [c1y, c0y];
      }

      const chunkCount = (c1x - c0x + 1) * (c1y - c0y + 1);
      if (chunkCount > MAX_CHUNKS) {
        ctx.fillStyle = "#c9a227";
        ctx.font = "14px system-ui, sans-serif";
        ctx.fillText(
          `Слишком много чанков (${chunkCount}). Приблизьте колёсиком.`,
          12,
          24,
        );
        return;
      }

      const promises: Promise<void>[] = [];
      for (let ccy = c0y; ccy <= c1y; ccy++) {
        for (let ccx = c0x; ccx <= c1x; ccx++) {
          promises.push(
            (async () => {
              const bmp = await getChunkBitmap(ccx, ccy);
              const wx0 = ccx * CHUNK;
              const wy0 = ccy * CHUNK;
              const sx = (wx0 - wl) * zoom;
              const sy = (wy0 - wt) * zoom;
              ctx.imageSmoothingEnabled = false;
              ctx.drawImage(bmp, sx, sy, CHUNK * zoom, CHUNK * zoom);
            })(),
          );
        }
      }
      await Promise.all(promises);
    } catch (e) {
      metaEl.textContent = `Ошибка карты: ${e}. Проверьте VITE_API_BASE / VITE_MAPVIEWER_TOKEN и порт mapviewer на сервере.`;
    }
  }

  await drawFrame();
}

void main();
