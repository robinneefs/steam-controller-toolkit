/** Scrolling line graph on a canvas, with a legend that shows live or hovered values. */

export interface Series {
  name: string;
  /** CSS custom property holding the line color, e.g. "--series-1". */
  color: string;
}

export interface GraphOptions {
  title: string;
  unit: string;
  series: Series[];
  /** Fixed y range. Omit for a symmetric range that grows to fit the data. */
  range?: [number, number];
  /** Smallest half-range for auto ranges. */
  minSpan?: number;
  /** Auto range starts at zero instead of being symmetric (for non-negative data). */
  positive?: boolean;
  decimals?: number;
  /**
   * "sqrt" compresses large values (signed square root) so a fixed full-scale range still shows
   * small motions. Needs `range`. `ticks` are the positive values to draw labelled gridlines at.
   */
  scale?: "linear" | "sqrt";
  ticks?: number[];
}

const SAMPLES = 300; // 5 s at 60 snapshots per second

export class LiveGraph {
  private canvas: HTMLCanvasElement;
  private values: HTMLElement[] = [];
  private data: Float32Array[];
  private head = 0;
  private filled = 0;
  private hoverX: number | null = null;
  private span: number;

  constructor(container: HTMLElement, private opts: GraphOptions) {
    container.classList.add("graph");
    const legend = opts.series
      .map((s, i) => `<span class="legend-item"><i style="background: var(${s.color})"></i>${s.name} <b data-v="${i}" data-copy></b></span>`)
      .join("");
    container.innerHTML = `
      <div class="graph-head"><span class="graph-title">${opts.title} <small>${opts.unit}</small></span>${legend}</div>
      <canvas></canvas>`;
    this.canvas = container.querySelector("canvas")!;
    this.values = opts.series.map((_, i) => container.querySelector<HTMLElement>(`[data-v="${i}"]`)!);
    this.data = opts.series.map(() => new Float32Array(SAMPLES));
    this.span = opts.minSpan ?? 1;

    this.canvas.addEventListener("mousemove", (e) => {
      this.hoverX = e.offsetX;
      this.draw();
    });
    this.canvas.addEventListener("mouseleave", () => {
      this.hoverX = null;
      this.draw();
    });
    new ResizeObserver(() => this.draw()).observe(this.canvas);
  }

  push(values: number[]): void {
    values.forEach((v, i) => (this.data[i][this.head] = v));
    this.head = (this.head + 1) % SAMPLES;
    this.filled = Math.min(this.filled + 1, SAMPLES);
    this.draw();
  }

  /** Sample `i` positions back from the newest (0 = newest). */
  private at(series: number, back: number): number {
    return this.data[series][(this.head - 1 - back + SAMPLES * 2) % SAMPLES];
  }

  private yRange(): [number, number] {
    if (this.opts.range) return this.opts.range;
    let peak = 0;
    for (let s = 0; s < this.data.length; s++) {
      for (let i = 0; i < this.filled; i++) peak = Math.max(peak, Math.abs(this.at(s, i)));
    }
    // Grow immediately, shrink slowly so the scale doesn't jump around.
    const target = Math.max(this.opts.minSpan ?? 1, niceCeil(peak));
    this.span = target > this.span ? target : this.span + (target - this.span) * 0.02;
    return this.opts.positive ? [0, this.span] : [-this.span, this.span];
  }

  private draw(): void {
    const c = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    if (!w || !h) return;
    if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
      c.width = Math.round(w * dpr);
      c.height = Math.round(h * dpr);
    }
    const ctx = c.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const css = getComputedStyle(c);
    const [lo, hi] = this.yRange();
    const curve = this.opts.scale === "sqrt" ? (v: number) => Math.sign(v) * Math.sqrt(Math.abs(v)) : (v: number) => v;
    const [cLo, cHi] = [curve(lo), curve(hi)];
    const y = (v: number) => h - 2 - ((curve(v) - cLo) / (cHi - cLo)) * (h - 4);
    const x = (back: number) => w - (back / (SAMPLES - 1)) * w;

    // Recessive grid: baseline at zero (or bottom), plus top and bottom labels.
    ctx.strokeStyle = css.getPropertyValue("--grid");
    ctx.lineWidth = 1;
    ctx.beginPath();
    const zero = lo < 0 && hi > 0 ? 0 : lo;
    ctx.moveTo(0, Math.round(y(zero)) + 0.5);
    ctx.lineTo(w, Math.round(y(zero)) + 0.5);
    ctx.stroke();
    ctx.fillStyle = css.getPropertyValue("--muted");
    ctx.font = "10px system-ui, sans-serif";
    ctx.textBaseline = "top";
    ctx.fillText(fmt(hi, 0), 2, 1);
    ctx.textBaseline = "bottom";
    ctx.fillText(fmt(lo, 0), 2, h - 1);

    // Labelled gridlines, mirrored for symmetric ranges.
    ctx.textBaseline = "middle";
    for (const t of this.opts.ticks ?? []) {
      for (const v of lo < 0 ? [t, -t] : [t]) {
        if (v <= lo || v >= hi) continue;
        const py = Math.round(y(v)) + 0.5;
        ctx.strokeStyle = css.getPropertyValue("--grid");
        ctx.setLineDash([2, 4]);
        ctx.beginPath();
        ctx.moveTo(28, py);
        ctx.lineTo(w, py);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillText(fmt(v, 0), 2, py);
      }
    }

    ctx.lineWidth = 2;
    ctx.lineJoin = "round";
    this.opts.series.forEach((s, si) => {
      ctx.strokeStyle = css.getPropertyValue(s.color);
      ctx.beginPath();
      for (let i = 0; i < this.filled; i++) {
        const px = x(i);
        const py = y(Math.min(hi, Math.max(lo, this.at(si, i))));
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.stroke();
    });

    // Hover crosshair: legend shows the values under the cursor instead of the newest.
    let back = 0;
    if (this.hoverX !== null && this.filled > 0) {
      back = Math.min(this.filled - 1, Math.max(0, Math.round(((w - this.hoverX) / w) * (SAMPLES - 1))));
      ctx.strokeStyle = css.getPropertyValue("--muted");
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(Math.round(x(back)) + 0.5, 0);
      ctx.lineTo(Math.round(x(back)) + 0.5, h);
      ctx.stroke();
    }
    const d = this.opts.decimals ?? 0;
    this.values.forEach((el, i) => (el.textContent = this.filled ? fmt(this.at(i, back), d) : "–"));
  }
}

function niceCeil(v: number): number {
  if (v <= 0) return 0;
  const p = 10 ** Math.floor(Math.log10(v));
  for (const m of [1, 2, 2.5, 5, 10]) if (v <= m * p) return m * p;
  return 10 * p;
}

function fmt(v: number, decimals: number): string {
  return v.toFixed(decimals).replace(/^-(0\.?0*)$/, "$1");
}
