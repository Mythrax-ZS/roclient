//! Granny's wavelet texture codec (texture encoding 3, "Bink" in Granny's terms): how every
//! texture inside RO's .gr2 models is stored.
//!
//! Each colour plane is a four-level subband pyramid, its coefficients arithmetic-coded with
//! adaptive models (signs and escapes in a separate raw bit stream), rebuilt by inverse
//! lifting filters, then turned from a luma / chroma-difference triple into RGB.
//!
//! Ported from granny-ro-js (dist/granny-ro-igc.js, v1.5.0), MIT licensed:
//! Copyright (c) 2026 Stitchuuuu. JavaScript's int32 wrapping and Int16Array stores are kept
//! as explicit casts, since the output is meant to match granny2.dll byte for byte.

use anyhow::{bail, ensure, Result};

/// Decodes a `width` x `height` image to RGBA.
pub fn decode(data: &[u8], width: u32, height: u32, alpha: bool) -> Result<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w * h <= 256 {
        ensure!(data.len() >= w * h * 4, "igc: small image needs {} RGBA bytes", w * h * 4);
        return Ok(data[..w * h * 4].to_vec());
    }
    ensure!(w >= 16 && h >= 16 && w % 16 == 0 && h % 16 == 0, "igc: {w}x{h} is not a multiple of 16");
    ensure!(w * h <= 4096 * 4096, "igc: {w}x{h} too large");
    let planes_n = if alpha { 4 } else { 3 };
    let mut planes = vec![vec![0i16; w * h]; 4];
    let mut flags = vec![0u8; h];
    let mut scratch = vec![0i16; w * h];
    let mut at = 4usize;
    for (p, plane) in planes.iter_mut().enumerate().take(planes_n) {
        at += decode_plane(data, at, plane, w, h, if p == 0 { Some(&mut flags) } else { None })?;
        inverse(plane, w * 8, w >> 3, h >> 3, None, &mut scratch);
        inverse(plane, w * 4, w >> 2, h >> 2, None, &mut scratch);
        inverse(plane, w * 2, w >> 1, h >> 1, None, &mut scratch);
        inverse(plane, w, w, h, if p == 0 { Some(&flags) } else { None }, &mut scratch);
    }
    if !alpha {
        planes[3].fill(255);
    }
    let mut out = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let (mut r, mut g, mut b) = (planes[1][i] as i32, planes[0][i] as i32, planes[2][i] as i32);
        g -= (r + b) / 4;
        r += g;
        b += g;
        let a = planes[3][i] as i32;
        out[i * 4..i * 4 + 4].copy_from_slice(&[r.clamp(0, 255) as u8, g.clamp(0, 255) as u8, b.clamp(0, 255) as u8, a.clamp(0, 255) as u8]);
    }
    Ok(out)
}

fn u32_at(buf: &[u8], o: usize) -> u32 {
    let mut b = [0u8; 4];
    for (i, x) in b.iter_mut().enumerate() {
        *x = buf.get(o + i).copied().unwrap_or(0);
    }
    u32::from_le_bytes(b)
}

/// One plane's subbands, coarsest first; returns the bytes it took.
fn decode_plane(buf: &[u8], off: usize, plane: &mut [i16], w: usize, h: usize, flags: Option<&mut Vec<u8>>) -> Result<usize> {
    let arith_len = u32_at(buf, off) as usize;
    let mut ar = Arith::new(buf, off + 8);
    let mut bits = Bits { buf, cur: off + 8 + arith_len, bits: 0, bitlen: 0 };
    decode_low(&mut ar, &mut bits, plane, 0, w * 16, w >> 4, h >> 4);
    for level in [4u32, 3, 2, 1] {
        let (stride, bw, bh) = (w << level, w >> level, h >> level);
        let half_row = w << (level - 1);
        for o in [bw, half_row, bw + half_row] {
            decode_high(&mut ar, &mut bits, plane, o, stride, bw, bh)?;
        }
    }
    if let Some(flags) = flags {
        let threshold = ar.get_value(h as u32 + 1);
        for f in flags.iter_mut().take(h) {
            if ar.get_count(h as u32) >= threshold {
                *f = 1;
                ar.remove(threshold, h as u32 - threshold, h as u32);
            } else {
                *f = 0;
                ar.remove(0, threshold, h as u32);
            }
        }
    }
    Ok(arith_len + u32_at(buf, off + 4) as usize + 8)
}

fn fill(dst: &mut [i16], start: usize, stride: usize, w: usize, h: usize, v: i32) {
    for y in 0..h {
        for x in 0..w {
            if let Some(d) = dst.get_mut(start + y * stride + x) {
                *d = v as i16;
            }
        }
    }
}

fn sext16(v: u32) -> i32 {
    v as u16 as i16 as i32
}

/// A symbol from `model`; escapes (first sightings) are read with `escape`.
fn symbol(model: &mut Model, ar: &mut Arith, escape: impl FnOnce(&mut Arith) -> u32) -> u32 {
    match model.decode(ar) {
        Sym::Value(v) => v as u32,
        Sym::Escape(slot) => {
            let v = escape(ar);
            model.values[slot] = v as u16;
            v
        }
    }
}

fn signed(v: u32, bits: &mut Bits) -> i32 {
    let mut v = v as i32;
    if v != 0 {
        let s = -(bits.get1() as i32);
        v = (v ^ s) - s;
    }
    v
}

/// The coarsest band: deltas from the left / above neighbours.
fn decode_low(ar: &mut Arith, bits: &mut Bits, dst: &mut [i16], mut o: usize, stride: usize, w: usize, h: usize) {
    if bits.get1() != 0 {
        let v = sext16(bits.get(16));
        fill(dst, o, stride, w, h, v);
        return;
    }
    let max = bits.get(16);
    let unique = max + 1;
    let mut model = Model::new(unique);
    let gap = stride - w;
    let mut acc = sext16(bits.get(16));
    dst[o] = acc as i16;
    o += 1;
    for _ in 0..w.saturating_sub(1) {
        let v = signed(symbol(&mut model, ar, |a| a.get_value(unique)), bits);
        acc = v.wrapping_add(acc);
        dst[o] = acc as i16;
        o += 1;
    }
    for _ in 0..h.saturating_sub(1) {
        o += gap;
        let mut above = o - stride;
        let v = signed(symbol(&mut model, ar, |a| a.get_value(unique)), bits);
        acc = v.wrapping_add(dst[above] as i32);
        dst[o] = acc as i16;
        o += 1;
        above += 1;
        for _ in 0..w.saturating_sub(1) {
            let v = signed(symbol(&mut model, ar, |a| a.get_value(unique)), bits);
            acc = v.wrapping_add(acc.wrapping_add(dst[above] as i32) / 2);
            dst[o] = acc as i16;
            o += 1;
            above += 1;
        }
    }
}

/// Bit length (0..15) of a context magnitude.
fn magnitude_class(v: u32) -> usize {
    if v == 0 {
        0
    } else if v >= 16384 {
        15
    } else {
        32 - v.leading_zeros() as usize
    }
}

const LIT_RUNS: [u32; 4] = [128, 256, 512, 1024];
const ZERO_RUNS: [u32; 4] = [512, 1024, 2048, 3072];

/// A detail band: runs of coded coefficients and runs of zeros, each coefficient's model
/// picked by its neighbours' magnitudes.
fn decode_high(ar: &mut Arith, bits: &mut Bits, dst: &mut [i16], start: usize, stride: usize, w: usize, h: usize) -> Result<()> {
    let quant = bits.get(16) as i32;
    if bits.get1() != 0 {
        let v = sext16(bits.get(16));
        fill(dst, start, stride, w, h, v.wrapping_mul(quant));
        return Ok(());
    }
    let max = bits.get(16);
    let unique = max + 1;
    let classes = magnitude_class(max.wrapping_mul(quant as u32)) + 1;
    let mut contexts: Vec<Model> = (0..classes).map(|_| Model::new(unique)).collect();
    let mut lits = Model::new(64);
    let mut zeros = Model::new(256);
    let gap = stride - w;
    let mut rows = h;
    let mut coef = |ar: &mut Arith, bits: &mut Bits, ctx: usize| -> i32 {
        let v = symbol(&mut contexts[ctx.min(classes - 1)], ar, |a| a.get_value(unique));
        signed(v, bits).wrapping_mul(quant)
    };
    let mut u = start;
    // The first coefficient is coded flat, before any context exists.
    let first = signed(ar.get_value(unique), bits).wrapping_mul(quant);
    dst[u] = first as i16;
    let (mut e, mut d, mut o) = (first, first, first);
    let mut k = u;
    u += 1;
    let mut left; // coefficients left in this row
    if w == 1 {
        rows -= 1;
        if rows == 0 {
            return Ok(());
        }
        left = w;
        u += gap;
        k = u - stride;
        e = dst[k] as i32;
        k += 1;
        d = e;
        o = e;
    } else {
        left = w - 1;
    }
    let mut stuck = 0;
    loop {
        let mut lit = symbol(&mut lits, ar, |_| bits.get(6));
        if lit >= 60 {
            lit = LIT_RUNS[(lit - 60) as usize];
        }
        let mut zero = symbol(&mut zeros, ar, |_| bits.get(8));
        if zero >= 252 {
            zero = ZERO_RUNS[(zero - 252) as usize] + 2;
        } else if zero != 0 {
            zero += 2;
        }
        if lit == 0 && zero == 0 {
            stuck += 1;
            if stuck > 64 {
                bail!("igc: detail band stuck (bitstream off its rails)");
            }
        } else {
            stuck = 0;
        }
        while lit > 0 {
            if left <= 1 {
                if left == 1 {
                    let c = ((o.wrapping_mul(2) as i64).abs() + (d as i64).abs() + (e as i64).abs()) as u32 / 4;
                    let v = coef(ar, bits, magnitude_class(c));
                    dst[u] = v as i16;
                    u += 1;
                    lit -= 1;
                }
                rows -= 1;
                if rows == 0 {
                    return Ok(());
                }
                left = w;
                u += gap;
                k = u - stride;
                e = dst[k] as i32;
                k += 1;
                d = e;
                o = e;
            } else {
                let above = dst[k] as i32;
                let c = ((o as i64).abs() + (d as i64).abs() + (e as i64).abs() + (above as i64).abs()) as u32 / 4;
                let v = coef(ar, bits, magnitude_class(c));
                dst[u] = v as i16;
                d = e;
                e = above;
                o = v;
                u += 1;
                k += 1;
                left -= 1;
                lit -= 1;
            }
        }
        while zero > 0 {
            if zero as usize >= left {
                zero -= left as u32;
                for _ in 0..left {
                    dst[u] = 0;
                    u += 1;
                }
                rows -= 1;
                if rows == 0 {
                    return Ok(());
                }
                left = w;
                u += gap;
                k = u - stride;
                e = dst[k] as i32;
                k += 1;
                d = e;
                o = e;
            } else {
                left -= zero as usize;
                k += zero as usize;
                for _ in 0..zero {
                    dst[u] = 0;
                    u += 1;
                }
                zero = 0;
                o = 0;
                e = dst[k - 1] as i32;
                d = dst[k - 2] as i32;
            }
        }
    }
}

// ---------------------------------------------------------------- inverse wavelet

/// JavaScript's `x | 0` on an integer-valued double.
fn ti32(x: i64) -> i64 {
    x as i32 as i64
}

/// The filters' rounding shift: `(x + (32767 ^ x >> 31)) / 65536 | 0`.
fn round16(x: i64) -> i16 {
    let sign = ((x as i32) >> 31) as i64;
    ((x + (32767 ^ sign)) / 65536) as i32 as i16
}

/// The 4-tap synthesis pair at window offset `j`: (even, odd) outputs from low taps `m`
/// and, when present, high taps `h`.
fn synth(m: &[i64], h: Option<&[i64]>, j: usize) -> (i16, i16) {
    let mut even = m[1 + j] * 51674 - ti32((m[j] + m[2 + j]) * 2667);
    let mut odd = ti32((m[1 + j] + m[2 + j]) * 27400) - ti32((m[j] + m[3 + j]) * 4230);
    if let Some(h) = h {
        even += -ti32((h[j] + h[3 + j]) * 1563) + ti32((h[1 + j] + h[2 + j]) * 24733);
        odd += -ti32(h[2 + j] * 55882) - ti32((h[j] + h[4 + j]) * 2479) + ti32((h[1 + j] + h[3 + j]) * 7250);
    }
    (round16(even), round16(odd))
}

fn at(v: &[i16], i: isize) -> i64 {
    if i < 0 {
        0
    } else {
        v.get(i as usize).copied().unwrap_or(0) as i64
    }
}

/// One level: rows (low half, high half) into `scratch`, then columns back into `plane`,
/// interleaved in strips so the scratch rows are ready before the columns need them.
fn inverse(plane: &mut [i16], stride: usize, w: usize, h: usize, flags: Option<&[u8]>, scratch: &mut [i16]) {
    let horiz = if w >= 16 { horizontal4 } else { horizontal2 };
    let vert = if h >= 16 { vertical4 } else { vertical2 };
    let mut done = if h <= 16 { h } else { 12 };
    let mut rest = h - done;
    let mut row = 0;
    let mut left = h;
    horiz(scratch, stride, plane, stride, w, h, flags, 0, done);
    while left > 0 {
        let n = if left <= 12 { left } else { 8 };
        vert(plane, stride, scratch, stride, w, h, row, n);
        row += n;
        left -= n;
        if rest > 0 {
            let n = if rest <= 12 { rest } else { 8 };
            horiz(scratch, stride, plane, stride, w, h, flags, done, n);
            done += n;
            rest -= n;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn horizontal4(dst: &mut [i16], dst_stride: usize, src: &[i16], src_stride: usize, width: usize, _h: usize, flags: Option<&[u8]>, start: usize, rows: usize) {
    let half = width >> 1;
    let (mut u, mut d, mut f, mut p) = (start * dst_stride, start * src_stride, start * src_stride + half, start);
    for _ in 0..rows {
        let mut step: isize = 1;
        let mut a = u;
        let mut s = d as isize;
        let mut c = f as isize;
        let g = (d + half) as isize;
        let mut m = [0i64; 7];
        let mut hi = [0i64; 8];
        m[1] = at(src, s);
        m[0] = at(src, s + 1);
        m[2] = m[0];
        for i in 3..7 {
            m[i] = at(src, s + i as isize - 1);
        }
        s += 6;
        for i in 2..8 {
            hi[i] = at(src, c + i as isize - 2);
        }
        hi[0] = hi[3];
        hi[1] = hi[2];
        c += 6;
        let detail = flags.is_none_or(|fl| fl.get(p).copied().unwrap_or(0) != 0);
        let mut quads = if half < 8 { 0 } else { (half - 8) / 4 };
        let tail = if half < 8 { half } else { (half & 3) + 8 };
        if detail {
            while quads > 0 {
                quads -= 1;
                for j in 0..4 {
                    let (e, o) = synth(&m, Some(&hi), j);
                    dst[a + j * 2] = e;
                    dst[a + j * 2 + 1] = o;
                }
                a += 8;
                m[0] = m[4];
                m[1] = m[5];
                m[2] = m[6];
                for i in 0..4 {
                    m[3 + i] = at(src, s + i as isize);
                }
                for i in 0..4 {
                    hi[i] = hi[i + 4];
                    hi[4 + i] = at(src, c + i as isize);
                }
                s += 4;
                c += 4;
            }
            for _ in 0..tail {
                if s == g {
                    s -= step;
                    c -= step + step;
                    step = -step;
                }
                let (e, o) = synth(&m, Some(&hi), 0);
                dst[a] = e;
                dst[a + 1] = o;
                a += 2;
                m.copy_within(1..7, 0);
                m[6] = at(src, s);
                hi.copy_within(1..8, 0);
                hi[7] = at(src, c);
                s += step;
                c += step;
            }
        } else {
            while quads > 0 {
                quads -= 1;
                for j in 0..4 {
                    let (e, o) = synth(&m, None, j);
                    dst[a + j * 2] = e;
                    dst[a + j * 2 + 1] = o;
                }
                a += 8;
                m[0] = m[4];
                m[1] = m[5];
                m[2] = m[6];
                for i in 0..4 {
                    m[3 + i] = at(src, s + i as isize);
                }
                s += 4;
            }
            for _ in 0..tail {
                if s == g {
                    s -= 1;
                    step = -1;
                }
                let (e, o) = synth(&m, None, 0);
                dst[a] = e;
                dst[a + 1] = o;
                a += 2;
                m.copy_within(1..7, 0);
                m[6] = at(src, s);
                s += step;
            }
        }
        u += dst_stride;
        d += src_stride;
        f += src_stride;
        p += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn horizontal2(dst: &mut [i16], dst_stride: usize, src: &[i16], src_stride: usize, width: usize, _h: usize, flags: Option<&[u8]>, start: usize, rows: usize) {
    let half = width >> 1;
    let (mut u, mut d, mut f, mut p) = (start * dst_stride, start * src_stride, start * src_stride + half, start);
    for _ in 0..rows {
        let (mut i, mut a, mut s) = (u, d, f);
        let detail = flags.is_none_or(|fl| fl.get(p).copied().unwrap_or(0) != 0);
        for _ in 0..half {
            let lo = src[a] as i32;
            if detail {
                let hv = src[s] as i32;
                let (x, y) = (half_round(lo.wrapping_mul(2).wrapping_add(hv)), half_round(lo.wrapping_mul(2).wrapping_sub(hv)));
                dst[i] = x as i16;
                dst[i + 1] = y as i16;
            } else {
                dst[i] = lo as i16;
                dst[i + 1] = lo as i16;
            }
            i += 2;
            a += 1;
            s += 1;
        }
        u += dst_stride;
        d += src_stride;
        f += src_stride;
        p += 1;
    }
}

/// `(x + (1 ^ x >> 31)) / 2 | 0`.
fn half_round(x: i32) -> i32 {
    ((x as i64 + (1 ^ (x >> 31)) as i64) / 2) as i32
}

#[allow(clippy::too_many_arguments)]
fn vertical2(dst: &mut [i16], dst_stride: usize, src: &[i16], src_stride: usize, width: usize, _h: usize, start: usize, rows: usize) {
    let pairs = rows >> 1;
    let base = start * dst_stride;
    let step = src_stride * 2;
    let (mut lo0, mut hi0) = (0usize, src_stride);
    if start > 0 {
        lo0 += (start / 2) * step;
        hi0 += (start / 2) * step;
    }
    for col in 0..width {
        let (mut i, mut a, mut o) = (base + col, lo0 + col, hi0 + col);
        for _ in 0..pairs {
            let (lo, hv) = (src[a] as i32, src[o] as i32);
            dst[i] = half_round(lo.wrapping_mul(2).wrapping_add(hv)) as i16;
            dst[i + dst_stride] = half_round(lo.wrapping_mul(2).wrapping_sub(hv)) as i16;
            i += dst_stride * 2;
            a += step;
            o += step;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn vertical4(dst: &mut [i16], dst_stride: usize, src: &[i16], src_stride: usize, width: usize, height: usize, start: usize, rows: usize) {
    let pairs = rows >> 1;
    let out0 = start * dst_stride;
    let step = src_stride as isize * 2;
    let (mut lo_base, mut hi_base) = (0isize, src_stride as isize);
    let end_base = (src_stride * height) as isize;
    if start > 0 {
        lo_base += ((start / 2) as isize - 1) * step;
        hi_base += ((start / 2) as isize - 2) * step;
    }
    let mut col = 0usize;
    // Four columns at a time, then the rest one by one.
    for _ in 0..width / 4 {
        let mut r = step;
        let mut i = out0 + col;
        let mut a = lo_base + col as isize;
        let mut s = hi_base + col as isize;
        let end = end_base + col as isize;
        // Low window g[20 rows][4 cols], high window v[21 rows][4 cols].
        // (A strip is at most 12 rows, so the window never reaches its end and compacts.)
        let mut g = [[0i64; 4]; 21];
        let mut v = [[0i64; 4]; 22];
        let row4 = |p: isize| -> [i64; 4] { std::array::from_fn(|e| at(src, p + e as isize)) };
        if start > 0 {
            for k in 0..4 {
                g[k] = row4(a);
                a += r;
            }
            for k in 0..5 {
                v[k] = row4(s);
                s += r;
            }
        } else {
            g[1] = row4(a);
            a += r;
            g[0] = row4(a);
            g[2] = g[0];
            a += r;
            g[3] = row4(a);
            a += r;
            v[1] = row4(s);
            v[2] = v[1];
            s += r;
            v[0] = row4(s);
            v[3] = v[0];
            s += r;
            v[4] = row4(s);
            s += r;
        }
        let (mut y, mut sy) = (0usize, 0usize);
        for _ in 0..pairs {
            if a == end {
                a -= r;
                s -= r + r;
                r = -r;
            }
            for e in 0..4 {
                let m = [g[y][e], g[y + 1][e], g[y + 2][e], g[y + 3][e]];
                let hh = [v[sy][e], v[sy + 1][e], v[sy + 2][e], v[sy + 3][e], v[sy + 4][e]];
                let (ev, od) = synth_v(&m, &hh);
                dst[i + e] = ev;
                dst[i + dst_stride + e] = od;
            }
            i += dst_stride * 2;
            y += 1;
            sy += 1;
            if y + 3 == 20 {
                g[0] = g[y + 1];
                g[1] = g[y + 2];
                g[2] = g[y + 3];
                v[0] = v[sy + 1];
                v[1] = v[sy + 2];
                v[2] = v[sy + 3];
                v[3] = v[sy + 4];
                y = 0;
                sy = 0;
            }
            g[y + 3] = row4(a);
            v[sy + 4] = row4(s);
            a += r;
            s += r;
        }
        col += 4;
    }
    for _ in 0..(width & 3) {
        let mut r = step;
        let mut i = out0 + col;
        let mut a = lo_base + col as isize;
        let mut s = hi_base + col as isize;
        let end = end_base + col as isize;
        let (mut g, mut v) = ([0i64; 4], [0i64; 5]);
        if start > 0 {
            for k in 0..4 {
                g[k] = at(src, a);
                a += r;
            }
            for k in 0..5 {
                v[k] = at(src, s);
                s += r;
            }
        } else {
            g[1] = at(src, a);
            a += r;
            g[0] = at(src, a);
            g[2] = g[0];
            a += r;
            g[3] = at(src, a);
            a += r;
            v[1] = at(src, s);
            v[2] = v[1];
            s += r;
            v[0] = at(src, s);
            v[3] = v[0];
            s += r;
            v[4] = at(src, s);
            s += r;
        }
        for _ in 0..pairs {
            if a == end {
                a -= r;
                s -= r + r;
                r = -r;
            }
            let (ev, od) = synth_v(&g, &v);
            dst[i] = ev;
            dst[i + dst_stride] = od;
            i += dst_stride * 2;
            g.copy_within(1..4, 0);
            g[3] = at(src, a);
            v.copy_within(1..5, 0);
            v[4] = at(src, s);
            a += r;
            s += r;
        }
        col += 1;
    }
}

/// The vertical filter pair: low taps t,n,r,i and high taps a,s,h,y,S.
fn synth_v(m: &[i64; 4], h: &[i64; 5]) -> (i16, i16) {
    let even = m[1] * 51674 - ti32((m[0] + m[2]) * 2667) - ti32((h[0] + h[3]) * 1563) + ti32((h[1] + h[2]) * 24733);
    let odd = ti32((m[1] + m[2]) * 27400) - ti32((m[0] + m[3]) * 4230) - ti32(h[2] * 55882) - ti32((h[0] + h[4]) * 2479) + ti32((h[1] + h[3]) * 7250);
    (round16(even), round16(odd))
}

// ---------------------------------------------------------------- bit and arithmetic decoding

/// The raw bit stream: signs, escapes and flat-band values, LSB first.
struct Bits<'a> {
    buf: &'a [u8],
    cur: usize,
    bits: u32,
    bitlen: u32,
}

impl Bits<'_> {
    fn get1(&mut self) -> u32 {
        if self.bitlen == 0 {
            let w = u32_at(self.buf, self.cur);
            self.cur += 4;
            self.bits = w >> 1;
            self.bitlen = 31;
            return w & 1;
        }
        let r = self.bits & 1;
        self.bits >>= 1;
        self.bitlen -= 1;
        r
    }

    fn get(&mut self, n: u32) -> u32 {
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        if self.bitlen < n {
            let w = u32_at(self.buf, self.cur);
            self.cur += 4;
            let v = (self.bits | w.checked_shl(self.bitlen).unwrap_or(0)) & mask;
            self.bits = w.checked_shr(n - self.bitlen).unwrap_or(0);
            self.bitlen = self.bitlen + 32 - n;
            return v;
        }
        let v = self.bits & mask;
        self.bits = self.bits.checked_shr(n).unwrap_or(0);
        self.bitlen -= n;
        v
    }
}

const MASK31: u32 = 0x7FFF_FFFF;

/// The range decoder (31-bit), its input read LSB first with bytes and nibbles reversed.
struct Arith<'a> {
    buf: &'a [u8],
    ptr: usize,
    accum: u32,
    bits_left: u32,
    high: u32,
    low: u32,
    target: u32,
}

fn reverse(v: u32, n: u32) -> u32 {
    (0..n).fold(0, |r, i| (r << 1) | ((v >> i) & 1))
}

impl<'a> Arith<'a> {
    fn new(buf: &'a [u8], off: usize) -> Self {
        let o = u32_at(buf, off);
        Self { buf, ptr: off + 4, accum: o >> 31, bits_left: 1, high: MASK31, low: 0, target: reverse(o & MASK31, 31) }
    }

    fn read(&mut self, t: u32) -> u32 {
        let (n, r) = (self.bits_left, self.accum);
        let i = if n < t {
            let s = u32_at(self.buf, self.ptr);
            let c = if n == 0 { s } else { (s << n) | r };
            self.accum = s >> (t - n);
            self.bits_left = n + 32 - t;
            self.ptr += 4;
            c & ((1 << t) - 1)
        } else {
            let v = r & ((1 << t) - 1);
            self.accum = r >> t;
            self.bits_left = n - t;
            v
        };
        if t == 8 || t == 4 {
            reverse(i, t)
        } else {
            i
        }
    }

    fn normalize(&mut self) {
        let (mut t, mut n, mut r) = (self.high, self.low, self.target);
        let mut i = n ^ t;
        if i & 0x4000_0000 == 0 {
            while i & 0x7F80_0000 == 0 {
                t = ((t << 8) | 255) & MASK31;
                n = (n << 8) & MASK31;
                let a = self.read(8);
                r = ((r << 8) | a) & MASK31;
                i = n ^ t;
            }
            if i & 0x7800_0000 == 0 {
                t = ((t << 4) | 15) & MASK31;
                n = (n << 4) & MASK31;
                let a = self.read(4);
                r = ((r << 4) | a) & MASK31;
                i = n ^ t;
            }
            while i & 0x4000_0000 == 0 {
                t = ((t << 1) | 1) & MASK31;
                n = (n << 1) & MASK31;
                let a = self.read(1);
                r = ((r << 1) | a) & MASK31;
                i = n ^ t;
            }
        }
        if n & 0x2000_0000 != 0 {
            while t & 0x2000_0000 == 0 {
                n = ((n & 0x1FFF_FFFF) << 1) & MASK31;
                t = ((t << 1) | 0x4000_0001) & MASK31;
                r ^= 0x2000_0000;
                let b = self.read(1);
                r = ((r << 1) | b) & MASK31;
                if n & 0x2000_0000 == 0 {
                    break;
                }
            }
        }
        self.high = t;
        self.low = n;
        self.target = r;
    }

    fn get_count(&self, scale: u32) -> u32 {
        let n = self.high.wrapping_sub(self.low).wrapping_add(1) as u64;
        let r = self.target.wrapping_sub(self.low).wrapping_add(1) as u64;
        let i = ((r * scale as u64).wrapping_sub(1) / n.max(1)) as u32;
        if i >= scale {
            scale.saturating_sub(1)
        } else {
            i
        }
    }

    fn remove(&mut self, start: u32, count: u32, scale: u32) {
        let i = self.high.wrapping_sub(self.low).wrapping_add(1) as u64;
        let scale = scale.max(1) as u64;
        let a = (i * (start + count) as u64 / scale) as u32;
        let o = (i * start as u64 / scale) as u32;
        let s = self.low;
        self.high = s.wrapping_add(a).wrapping_sub(1);
        self.low = s.wrapping_add(o);
        self.normalize();
    }

    fn get_value(&mut self, scale: u32) -> u32 {
        let i = self.get_count(scale);
        self.remove(i, 1, scale);
        i
    }
}

enum Sym {
    Value(u16),
    Escape(usize),
}

/// An adaptive frequency model over up to `unique` values; slot 0 is the escape.
struct Model {
    cum: [u16; 16],
    single: Vec<u16>,
    values: Vec<u16>,
    singles: usize,
    band: usize,
    shift: u32,
    bucket: usize,
    unique: usize,
}

impl Model {
    fn new(unique: u32) -> Self {
        let n = ((unique as usize) + 5) & !3;
        let mut m = Self { cum: [0; 16], single: vec![0; n], values: vec![0; n], singles: 0, band: 0, shift: 0, bucket: 0, unique: unique as usize };
        m.set_shift(unique as usize + 1);
        m.add(0, 0x30003);
        m
    }

    fn set_shift(&mut self, t: usize) {
        if t < 6 {
            self.bucket = 0;
            self.shift = 15;
            self.band = 0;
            return;
        }
        let (mut best, mut best_max) = (0u32, u32::MAX as usize);
        for e in 0..16u32 {
            let size = 1usize << e;
            let bins = t.div_ceil(size).min(16);
            let last = (t - (bins - 1) * size).max(size);
            if last < best_max {
                best = e;
                best_max = last;
            }
            if size > t {
                break;
            }
        }
        self.bucket = (1usize << best) & 0xFFFF;
        self.shift = best;
        self.band = (15 * self.bucket) & 0xFFFF;
    }

    fn bin(&self, i: usize) -> usize {
        if i >= self.band {
            15
        } else {
            i >> self.shift
        }
    }

    fn add(&mut self, t: usize, n: u32) {
        let r = n as u16;
        if t >= self.band {
            self.cum[15] = self.cum[15].wrapping_add(r);
            if let Some(s) = self.single.get_mut(t) {
                *s = s.wrapping_add(r);
            }
            return;
        }
        let mut i = t >> self.shift;
        if i & 1 != 0 {
            self.cum[i] = self.cum[i].wrapping_add(r);
            i += 1;
        }
        i >>= 1;
        for p in i..=7 {
            let k = p * 2;
            let v = ((self.cum[k + 1] as u32) << 16 | self.cum[k] as u32).wrapping_add(n);
            self.cum[k] = v as u16;
            self.cum[k + 1] = (v >> 16) as u16;
        }
        if let Some(s) = self.single.get_mut(t) {
            *s = s.wrapping_add(r);
        }
    }

    fn find(&mut self, t: u32) -> (usize, u32) {
        let c = &self.cum;
        let (mut r, mut i) = (0usize, 0u32);
        if t >= c[7] as u32 {
            i = c[7] as u32;
            r = 8;
        }
        if t >= c[r + 3] as u32 {
            i = c[r + 3] as u32;
            r += 4;
        }
        if t >= c[r + 1] as u32 {
            i = c[r + 1] as u32;
            r += 2;
        }
        if t >= c[r] as u32 {
            i = c[r] as u32;
            r += 1;
        }
        for e in r..16 {
            self.cum[e] = self.cum[e].wrapping_add(1);
        }
        let mut a = r * self.bucket;
        let mut o = i;
        let end = self.single.len().min(self.unique + 1);
        while a < end {
            let e = (o + self.single[a] as u32) & 0xFFFF;
            if t < e {
                return (a, o);
            }
            o = e;
            a += 1;
        }
        (a.saturating_sub(1).min(self.unique), o)
    }

    fn rescale(&mut self) {
        self.set_shift(self.singles + 1);
        let mut t = [0u32; 16];
        self.single[0] >>= 1;
        t[self.bin(0)] = self.single[0] as u32;
        let (mut max, mut max_at, mut a) = (0u16, 0usize, 1usize);
        while a <= self.singles {
            while self.single[a] <= 1 {
                if a >= self.singles {
                    self.single[a] = 0;
                    self.singles -= 1;
                    break;
                }
                self.single[a] = self.single[self.singles];
                self.single[self.singles] = 0;
                self.values[a] = self.values[self.singles];
                self.singles -= 1;
            }
            if a > self.singles {
                break;
            }
            self.single[a] >>= 1;
            let n = self.single[a];
            if n > max {
                max = n;
                max_at = a;
            }
            let b = self.bin(a);
            t[b] = t[b].wrapping_add(n as u32);
            a += 1;
        }
        if max > 0 {
            let mut n = if self.singles >= self.band { self.band } else { (self.singles >> self.shift) << self.shift };
            if n == 0 {
                n = 1;
            }
            if max_at != n {
                let (r2, a2) = (self.single[n] as u32, self.single[max_at] as u32);
                let o = a2.wrapping_sub(r2);
                let (s, c) = (self.bin(n), self.bin(max_at));
                t[s] = t[s].wrapping_add(o);
                t[c] = t[c].wrapping_sub(o);
                self.single[n] = a2 as u16;
                self.single[max_at] = r2 as u16;
                self.values.swap(n, max_at);
            }
        }
        if self.singles != self.unique && self.single[0] == 0 {
            self.single[0] = 2;
            let b = self.bin(0);
            t[b] = t[b].wrapping_add(2);
        }
        let mut n = 0u16;
        for (c, v) in self.cum.iter_mut().zip(t) {
            n = n.wrapping_add(v as u16);
            *c = n;
        }
    }

    fn decode(&mut self, ar: &mut Arith) -> Sym {
        if self.cum[15] >= 16384 {
            self.rescale();
        }
        let n = self.cum[15] as u32;
        let (r, i) = self.find(ar.get_count(n));
        let a = self.single[r] as u32;
        ar.remove(i, a, n);
        self.single[r] = self.single[r].wrapping_add(1);
        if r != 0 {
            return Sym::Value(self.values[r]);
        }
        self.singles = (self.singles + 1) & 0xFFFF;
        self.add(self.singles, 0x20002);
        if self.singles == self.unique {
            let t = self.single[0] as u32;
            let n = t.wrapping_neg() & 0xFFFF;
            self.add(0, ((t.wrapping_neg().wrapping_sub(1) & 0xFFFF) << 16) | n);
        }
        Sym::Escape(self.singles.min(self.values.len() - 1))
    }
}
