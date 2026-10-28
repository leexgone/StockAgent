/* ============================================================
 * 图表层 — 纯 SVG，无外部依赖；宽度自适应 + 十字准星
 * ============================================================ */
(function (W) {
  'use strict';
  const NS = 'http://www.w3.org/2000/svg';
  const css = n => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const esc = s => String(s).replace(/[&<>]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c]));

  function scale(series, pad, w, h) {
    let min = Math.min(...series), max = Math.max(...series);
    const r = (max - min) || Math.abs(max) * 0.02 || 1;
    min -= r * pad; max += r * pad;
    return {
      min, max,
      x: i => (series.length === 1 ? w / 2 : (i / (series.length - 1)) * w),
      y: v => h - ((v - min) / (max - min)) * h,
    };
  }
  function path(pts) { return pts.map((p, i) => (i ? 'L' : 'M') + p[0].toFixed(1) + ' ' + p[1].toFixed(1)).join(' '); }
  function smooth(pts) {
    // 轻量 catmull-rom → 三次贝塞尔，避免折线生硬
    let d = `M${pts[0][0].toFixed(1)} ${pts[0][1].toFixed(1)}`;
    for (let i = 0; i < pts.length - 1; i++) {
      const p0 = pts[i - 1] || pts[i], p1 = pts[i], p2 = pts[i + 1], p3 = pts[i + 2] || p2;
      const c1x = p1[0] + (p2[0] - p0[0]) / 6, c1y = p1[1] + (p2[1] - p0[1]) / 6;
      const c2x = p2[0] - (p3[0] - p1[0]) / 6, c2y = p2[1] - (p3[1] - p1[1]) / 6;
      d += `C${c1x.toFixed(1)} ${c1y.toFixed(1)} ${c2x.toFixed(1)} ${c2y.toFixed(1)} ${p2[0].toFixed(1)} ${p2[1].toFixed(1)}`;
    }
    return d;
  }
  let uid = 0;

  /* ---------- 主折线 / 面积图（带十字准星） ---------- */
  function line(host, cfg) {
    const el = typeof host === 'string' ? document.querySelector(host) : host;
    if (!el) return;
    const s = Object.assign({ height: 250, pad: .08, color: css('--accent'), fill: true, yFmt: v => v.toFixed(2), labels: null, series: [], tip: i => '', crosshair: true }, cfg);
    function draw() {
      const w = Math.max(240, el.clientWidth || 600), h = s.height;
      const padL = 0, padR = 0, padT = 12, padB = s.labels ? 20 : 6;
      const iw = w - padL - padR, ih = h - padT - padB;
      const id = 'lg' + (++uid);
      let svg = `<svg class="chart" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">`;
      svg += `<defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="${s.color}" stop-opacity=".18"/><stop offset="1" stop-color="${s.color}" stop-opacity="0"/></linearGradient></defs>`;
      const all = s.series.reduce((a, x) => a.concat(x.data), []);
      if (!all.length) { el.innerHTML = ''; return; }   // 空序列（空持仓态）不画，页面另行渲染引导
      const sc = scale(all, s.pad, iw, ih);
      // 横向网格 + Y 轴标签
      for (let g = 0; g <= 3; g++) {
        const v = sc.min + (sc.max - sc.min) * g / 3, y = sc.y(v) + padT;
        svg += `<line class="grid-line" x1="${padL}" x2="${w - padR}" y1="${y.toFixed(1)}" y2="${y.toFixed(1)}"/>`;
        svg += `<text x="${padL + 2}" y="${(y - 4).toFixed(1)}" fill="${css('--muted')}">${esc(s.yFmt(v))}</text>`;
      }
      s.series.forEach((x, si) => {
        const pts = x.data.map((v, i) => [sc.x(i) + padL, sc.y(v) + padT]);
        if (x.area !== false && s.fill && si === 0) svg += `<path d="${smooth(pts)} L${pts[pts.length - 1][0].toFixed(1)} ${ih + padT} L${pts[0][0].toFixed(1)} ${ih + padT}Z" fill="url(#${id})"/>`;
        svg += `<path d="${smooth(pts)}" fill="none" stroke="${x.color || s.color}" stroke-width="${x.width || 1.8}" ${x.dash ? `stroke-dasharray="${x.dash}"` : ''} stroke-linecap="round" stroke-linejoin="round"/>`;
        const last = pts[pts.length - 1];
        if (x.endDot !== false) svg += `<circle cx="${last[0].toFixed(1)}" cy="${last[1].toFixed(1)}" r="2.8" fill="${x.color || s.color}"/>`;
      });
      if (s.labels) {
        const step = Math.ceil(s.labels.length / Math.max(2, Math.floor(iw / 74)));
        s.labels.forEach((L, i) => {
          if (i % step) return;
          svg += `<text x="${(sc.x(i) + padL).toFixed(1)}" y="${h - 4}" text-anchor="middle" fill="${css('--muted')}">${esc(L)}</text>`;
        });
      }
      svg += `<line id="cx${id}" x1="0" x2="0" y1="${padT}" y2="${ih + padT}" stroke="${css('--border-2')}" stroke-width="1" opacity="0"/>`;
      svg += `</svg>`;
      el.innerHTML = svg + `<div class="crosshair-tip" id="ct${id}"></div>`;
      if (!s.crosshair) return;
      const svgEl = el.querySelector('svg'), tip = el.querySelector('#ct' + id), cx = el.querySelector('#cx' + id);
      svgEl.addEventListener('pointermove', e => {
        const r = svgEl.getBoundingClientRect();
        const rel = (e.clientX - r.left - padL) / iw;
        const i = Math.max(0, Math.min(s.series[0].data.length - 1, Math.round(rel * (s.series[0].data.length - 1))));
        const x = sc.x(i) + padL;
        cx.setAttribute('x1', x); cx.setAttribute('x2', x); cx.setAttribute('opacity', '1');
        tip.innerHTML = s.tip(i);
        tip.style.opacity = '1';
        const tw = tip.offsetWidth;
        tip.style.left = Math.max(4, Math.min(el.clientWidth - tw - 4, x - tw / 2)) + 'px';
        tip.style.top = (sc.y(s.series[0].data[i]) + padT - 10) + 'px';
        tip.style.transform = 'translateY(-100%)';
      });
      svgEl.addEventListener('pointerleave', () => { tip.style.opacity = '0'; cx.setAttribute('opacity', '0'); });
    }
    draw();
    let t; window.addEventListener('resize', () => { clearTimeout(t); t = setTimeout(draw, 140); });
  }

  /* ---------- 迷你走势线（表格内联） ---------- */
  function spark(data, dir, width, height) {
    width = width || 96; height = height || 30;
    if (!data || !data.length) return '';   // 无走势数据时留空（空态防御）
    const sc = scale(data, .12, width - 2, height - 4);
    const pts = data.map((v, i) => [sc.x(i) + 1, sc.y(v) + 2]);
    const col = dir >= 0 ? css('--up') : css('--down');
    const last = pts[pts.length - 1];
    return `<svg class="spark" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" aria-hidden="true">
      <path d="${smooth(pts)}" fill="none" stroke="${col}" stroke-width="1.5" stroke-linecap="round"/>
      <circle cx="${last[0].toFixed(1)}" cy="${last[1].toFixed(1)}" r="1.9" fill="${col}"/></svg>`;
  }

  /* ---------- 柱（量能） ---------- */
  function bars(data, dir, height) {
    height = height || 34;
    if (!data || !data.length) return '';   // 无量能数据时留空（空态防御）
    const w = 96, mx = Math.max(...data), gap = 1.6;
    const bw = w / data.length - gap;
    const col = dir >= 0 ? css('--up') : css('--down');
    let s = `<svg class="spark" viewBox="0 0 ${w} ${height}" width="${w}" height="${height}" aria-hidden="true">`;
    data.forEach((v, i) => {
      const h = Math.max(1.5, v / mx * (height - 2));
      s += `<rect x="${(i * (bw + gap)).toFixed(1)}" y="${(height - h).toFixed(1)}" width="${bw.toFixed(1)}" height="${h.toFixed(1)}" rx="1" fill="${col}" opacity="${0.28 + 0.6 * (v / mx)}"/>`;
    });
    return s + '</svg>';
  }

  /* ---------- 环形（行业权重） ---------- */
  const DONUT_COLORS = [
    'oklch(58% 0.18 255)', 'oklch(55% 0.19 22)', 'oklch(62% 0.15 78)',
    'oklch(56% 0.12 170)', 'oklch(60% 0.14 320)', 'oklch(52% 0.13 152)',
    'oklch(58% 0.1 285)', 'oklch(65% 0.06 250)',
  ];
  function donut(host, items, opts) {
    const el = typeof host === 'string' ? document.querySelector(host) : host;
    if (!el) return;
    opts = opts || {};
    const size = opts.size || 220, r = size / 2 - 14, sw = opts.thick || 22, cx = size / 2, cy = size / 2;
    const total = items.reduce((s, x) => s + x.v, 0);
    let a0 = -Math.PI / 2, svg = `<svg viewBox="0 0 ${size} ${size}" width="${size}" height="${size}" class="chart" style="max-width:100%">`;
    svg += `<circle cx="${cx}" cy="${cy}" r="${r}" fill="none" stroke="${css('--surface-2')}" stroke-width="${sw}"/>`;
    items.forEach((it, i) => {
      const frac = it.v / total, a1 = a0 + frac * Math.PI * 2 * 0.998;
      const large = frac > 0.5 ? 1 : 0;
      const p = (ang, rad) => [(cx + Math.cos(ang) * rad).toFixed(2), (cy + Math.sin(ang) * rad).toFixed(2)];
      const s0 = p(a0, r), s1 = p(a1, r);
      svg += `<path d="M${s0[0]} ${s0[1]} A${r} ${r} 0 ${large} 1 ${s1[0]} ${s1[1]}" fill="none"
        stroke="${it.color || DONUT_COLORS[i % DONUT_COLORS.length]}" stroke-width="${sw}" stroke-linecap="butt"
        data-seg="${esc(it.name)}"><title>${esc(it.name)} ${(frac * 100).toFixed(1)}%</title></path>`;
      a0 = a1;
    });
    svg += `</svg>`;
    el.innerHTML = svg;
  }

  /* ---------- 得分弧 ---------- */
  function gauge(host, score, label) {
    const el = typeof host === 'string' ? document.querySelector(host) : host;
    if (!el) return;
    const size = 132, r = 52, cx = size / 2, cy = size / 2 + 6, C0 = Math.PI * 1.5;
    const arc = (from, to) => {
      const p = a => [(cx + Math.cos(a) * r).toFixed(2), (cy + Math.sin(a) * r).toFixed(2)];
      const s0 = p(from), s1 = p(to);
      return `M${s0[0]} ${s0[1]} A${r} ${r} 0 ${to - from > Math.PI ? 1 : 0} 1 ${s1[0]} ${s1[1]}`;
    };
    const frac = Math.max(0, Math.min(1, score / 100));
    const col = score >= 70 ? css('--ok') : score >= 55 ? css('--warn') : css('--danger');   // 质量红绿灯（B5：得分不是涨跌）
    el.innerHTML = `<svg viewBox="0 0 ${size} ${size}" width="${size}" height="${size}" class="chart">
      <path d="${arc(C0, C0 + Math.PI)}" fill="none" stroke="${css('--surface-2')}" stroke-width="9" stroke-linecap="round"/>
      <path d="${arc(C0, C0 + Math.PI * frac)}" fill="none" stroke="${col}" stroke-width="9" stroke-linecap="round"/>
      <text x="${cx}" y="${cy + 2}" text-anchor="middle" style="font-family:var(--font-display);font-size:30px;font-weight:590;letter-spacing:-.03em" fill="${col}">${score}</text>
      <text x="${cx}" y="${cy + 22}" text-anchor="middle" style="font-size:10.5px;letter-spacing:.06em" fill="${css('--muted')}">${esc(label || '综合得分')}</text>
    </svg>`;
  }

  /* ---------- 水平条组 ---------- */
  function hbars(items, opts) {
    opts = opts || {};
    const max = Math.max(...items.map(i => i.v)) || 1;
    return items.map((it, i) => `
      <div class="bar-line" ${opts.id ? `data-od-id="${opts.id}-${i}"` : ''}>
        <span class="lb" title="${esc(it.k)}">${esc(it.k)}</span>
        <span class="tr"><i style="width:${(it.v / max * 100).toFixed(1)}%;background:${it.color || 'var(--fg-2)'}"></i></span>
        <span class="vl">${opts.fmt ? opts.fmt(it.v) : it.v.toFixed(1) + '%'}</span>
      </div>`).join('');
  }

  W.ZMC = { line, spark, bars, donut, gauge, hbars, smooth, path, scale, esc, DONUT_COLORS, color: css };
})(window);
