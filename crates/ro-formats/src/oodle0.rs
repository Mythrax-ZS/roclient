//! Oodle0, the compression of Granny 2 file sections (RO's .gr2 models): an adaptive
//! arithmetic coder driving LZ copies, in up to three blocks per section.
//!
//! Ported from the clean-room Python decoder in Rasetsuu/blendergranny
//! (io_scene_gr2/gr2/decompress/oodle0.py), MIT licensed:
//! Copyright (c) 2026 ciupix and contributors.

use anyhow::{bail, ensure, Result};

const HEADER_SIZE: usize = 36;
const OFFSET_SPLIT_SHIFT: u32 = 2;
const LOW_OFFSET_MASK: u32 = (1 << OFFSET_SPLIT_SHIFT) - 1;
const MAX_LENS: usize = 64;
const LONG_LENGTHS: [usize; 4] = [MAX_LENS * 2, MAX_LENS * 3, MAX_LENS * 4, MAX_LENS * 8];
const MASK31: u32 = 0x7FFF_FFFF;

/// Expands one section. `stop0` / `stop1` are the section's Oodle stops (where the 16-bit and
/// 8-bit data begin: the block boundaries).
pub fn decompress(src: &[u8], expanded: usize, stop0: u32, stop1: u32) -> Result<Vec<u8>> {
    if expanded == 0 {
        return Ok(Vec::new());
    }
    ensure!(src.len() >= HEADER_SIZE, "oodle0 data too short for its block headers");
    let word = |i: usize| u32::from_le_bytes(src[i * 4..i * 4 + 4].try_into().unwrap());
    let clamp = |v: u32| (v as usize).min(expanded);
    let s0 = clamp(stop0);
    let s1 = clamp(stop1).max(s0);
    let stops = [0, s0, s1, expanded];
    let mut bits = ArithBits::new(src, HEADER_SIZE);
    let mut out = Vec::with_capacity(expanded);
    for b in 0..3 {
        if stops[b + 1] <= stops[b] {
            continue;
        }
        let mut state = LzState::new(word(b * 3), word(b * 3 + 1), word(b * 3 + 2));
        decode_block(&mut state, &mut bits, &mut out, stops[b + 1])?;
    }
    ensure!(out.len() == expanded, "oodle0 decompressed {} bytes, expected {expanded}", out.len());
    Ok(out)
}

fn decode_block(state: &mut LzState, bits: &mut ArithBits, out: &mut Vec<u8>, stop: usize) -> Result<()> {
    while out.len() < stop {
        let prev = state.last_length;
        let sym = read_symbol(&mut state.lengths[prev], bits, MAX_LENS as u32 + 1)? as usize;
        state.last_length = sym.min(MAX_LENS);
        if sym != 0 {
            let len = if sym >= MAX_LENS - 3 { LONG_LENGTHS[(sym - (MAX_LENS - 3)).min(3)] } else { sym + 1 };
            let low = read_symbol(&mut state.offset_low, bits, state.max_offset_low)?;
            let high_scale = (state.max_offsets.min(state.decompressed) >> OFFSET_SPLIT_SHIFT) + 1;
            let high = read_symbol(&mut state.offset_high, bits, high_scale)?;
            let dist = (low + 1 + (high << OFFSET_SPLIT_SHIFT)) as usize;
            ensure!(dist > 0 && dist <= out.len(), "oodle0 copy distance {dist} out of range");
            for _ in 0..len {
                out.push(out[out.len() - dist]);
            }
            state.decompressed = state.decompressed.saturating_add(len as u32);
        } else {
            let lit = read_symbol(&mut state.bytes, bits, state.max_bytes)?;
            ensure!(lit <= 255, "oodle0 literal {lit} out of range");
            out.push(lit as u8);
            state.decompressed = state.decompressed.saturating_add(1);
        }
    }
    Ok(())
}

fn read_symbol(model: &mut ArithModel, bits: &mut ArithBits, escape_scale: u32) -> Result<u32> {
    match model.decompress(bits)? {
        Sym::Value(v) => Ok(v as u32),
        Sym::Escape(i) => {
            let v = bits.get_value(escape_scale);
            model.values[i] = v as u16;
            Ok(v)
        }
    }
}

struct LzState {
    max_bytes: u32,
    max_offsets: u32,
    max_offset_low: u32,
    bytes: ArithModel,
    lengths: Vec<ArithModel>,
    offset_low: ArithModel,
    offset_high: ArithModel,
    decompressed: u32,
    last_length: usize,
}

impl LzState {
    fn new(max_offset_and_byte: u32, uniq_offset_and_byte: u32, uniq_lens: u32) -> Self {
        let max_offsets = max_offset_and_byte >> 9;
        let max_offset_low = max_offsets.min(LOW_OFFSET_MASK + 1);
        let length_unique = |i: usize| {
            let group = (i / (MAX_LENS / 4)).min(3) as u32;
            (uniq_lens >> ((3 - group) * 8)) & 0xFF
        };
        Self {
            max_bytes: max_offset_and_byte & 0x1FF,
            max_offsets,
            max_offset_low,
            bytes: ArithModel::new(uniq_offset_and_byte & 0x1FF),
            lengths: (0..=MAX_LENS).map(|i| ArithModel::new(length_unique(i))).collect(),
            offset_low: ArithModel::new(max_offset_low),
            offset_high: ArithModel::new(uniq_offset_and_byte >> 9),
            decompressed: 0,
            last_length: 0,
        }
    }
}

/// Little-endian bit reader, refilled a 32-bit word at a time (zero past the end).
struct VarBits<'a> {
    data: &'a [u8],
    cur: usize,
    bits: u64,
    bitlen: u32,
}

impl VarBits<'_> {
    fn word(&mut self) -> u64 {
        let mut w = [0u8; 4];
        for (i, b) in w.iter_mut().enumerate() {
            *b = self.data.get(self.cur + i).copied().unwrap_or(0);
        }
        self.cur += 4;
        u32::from_le_bytes(w) as u64
    }

    fn get(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mask = (1u64 << n) - 1;
        if self.bitlen >= n {
            let v = self.bits & mask;
            self.bits >>= n;
            self.bitlen -= n;
            return v as u32;
        }
        let w = self.word();
        let v = (self.bits | (w << self.bitlen)) & mask;
        self.bits = w >> (n - self.bitlen);
        self.bitlen = self.bitlen + 32 - n;
        v as u32
    }

    fn get1(&mut self) -> u32 {
        if self.bitlen > 0 {
            let v = self.bits & 1;
            self.bits >>= 1;
            self.bitlen -= 1;
            return v as u32;
        }
        let w = self.word();
        self.bits = w >> 1;
        self.bitlen = 31;
        (w & 1) as u32
    }
}

fn bit_reverse(v: u32, n: u32) -> u32 {
    (0..n).fold(0, |r, i| (r << 1) | ((v >> i) & 1))
}

struct ArithBits<'a> {
    vbits: VarBits<'a>,
    high: u32,
    low: u32,
    code: u32,
}

impl<'a> ArithBits<'a> {
    fn new(data: &'a [u8], offset: usize) -> Self {
        let mut vbits = VarBits { data, cur: offset, bits: 0, bitlen: 0 };
        let code = bit_reverse(vbits.get(31), 31);
        Self { vbits, high: MASK31, low: 0, code }
    }

    fn get_count(&self, scale: u32) -> u32 {
        if scale == 0 {
            return 0;
        }
        let width = self.high.wrapping_sub(self.low) as u64 + 1;
        (((self.code.wrapping_sub(self.low) as u64 + 1) * scale as u64 - 1) / width) as u32
    }

    fn get_value(&mut self, scale: u32) -> u32 {
        let v = self.get_count(scale).min(scale.saturating_sub(1));
        self.remove(v, 1, scale);
        v
    }

    fn remove(&mut self, start: u32, count: u32, scale: u32) {
        if scale == 0 {
            return;
        }
        let (mut high, mut low, mut code) = (self.high, self.low, self.code);
        let width = high.wrapping_sub(low) as u64 + 1;
        high =(low as u64 + (width * (start + count) as u64) / scale as u64).wrapping_sub(1) as u32;
        low = (low as u64 + (width * start as u64) / scale as u64) as u32;
        if (high ^ low) & 0x4000_0000 == 0 {
            while (high ^ low) & 0x7F80_0000 == 0 {
                low <<= 8;
                high = (high << 8) | 0xFF;
                let byte = self.vbits.get(8);
                code = (code << 8) | (bit_reverse(byte & 0xF, 4) << 4) | bit_reverse(byte >> 4, 4);
            }
            if (high ^ low) & 0x7800_0000 == 0 {
                low <<= 4;
                high = (high << 4) | 0xF;
                code = (code << 4) | bit_reverse(self.vbits.get(4), 4);
            }
            while (high ^ low) & 0x4000_0000 == 0 {
                low <<= 1;
                high = (high << 1) | 1;
                code = (code << 1) | self.vbits.get1();
            }
        }
        while low & 0x2000_0000 != 0 && high & 0x2000_0000 == 0 {
            code ^= 0x2000_0000;
            low = (low & 0x1FFF_FFFF) << 1;
            high = (high << 1) | 0x4000_0001;
            code = (code << 1) | self.vbits.get1();
        }
        self.high = high & MASK31;
        self.low = low & MASK31;
        self.code = code & MASK31;
    }
}

enum Sym {
    Value(u16),
    Escape(usize),
}

/// An adaptive frequency model: slot 0 is the escape (a value not seen yet, sent raw).
struct ArithModel {
    unique: u32,
    totals: [u16; 16],
    counts: Vec<u16>,
    values: Vec<u16>,
    number: usize,
    bin_shift: u32,
    last_bin_start: usize,
}

fn best_shift(value: usize) -> (u32, usize) {
    if value < 6 {
        return (15, 0);
    }
    let (mut best_max, mut best_bin) = (usize::MAX, 0u32);
    for i in 0..16u32 {
        let size = 1usize << i;
        let bins = value.div_ceil(size).min(16);
        let last = (value - size * (bins - 1)).max(size);
        if last < best_max {
            best_bin = i;
            best_max = last;
        }
        if size > value {
            break;
        }
    }
    (best_bin, 15 * (1usize << best_bin))
}

impl ArithModel {
    fn new(unique: u32) -> Self {
        let n = ((unique as usize) + 5) & !3;
        let (bin_shift, last_bin_start) = best_shift(unique as usize + 1);
        let mut m = Self { unique, totals: [0; 16], counts: vec![0; n], values: vec![0; n], number: 0, bin_shift, last_bin_start };
        m.quick_increment(0, 0x30003);
        m
    }

    fn decompress(&mut self, bits: &mut ArithBits) -> Result<Sym> {
        if self.totals[15] >= 16384 {
            self.rescale();
        }
        let scale = self.totals[15] as u32;
        let count = bits.get_count(scale);
        let (pos, start) = self.find_pos(count)?;
        let old = self.counts[pos] as u32;
        self.increment_totals(pos, 0x10001);
        bits.remove(start, old, self.totals[15] as u32 - 1);
        self.counts[pos] = self.counts[pos].wrapping_add(1);
        if pos == 0 {
            self.number += 1;
            if self.number >= self.counts.len() {
                bail!("oodle0 escape exceeded model capacity");
            }
            self.quick_increment(self.number, 0x20002);
            if self.number == self.unique as usize {
                let c = self.counts[0] as u32;
                self.decrement_counts(0, c);
            }
            return Ok(Sym::Escape(self.number));
        }
        Ok(Sym::Value(self.values[pos]))
    }

    fn find_pos(&self, count: u32) -> Result<(usize, u32)> {
        let mut start = 0u32;
        for (pos, &c) in self.counts.iter().enumerate() {
            if count < start + c as u32 {
                return Ok((pos, start));
            }
            start += c as u32;
        }
        bail!("oodle0 model count {count} outside total {start}")
    }

    fn quick_increment(&mut self, value: usize, amount: u32) {
        self.increment_totals(value, amount);
        self.counts[value] = self.counts[value].wrapping_add(amount as u16);
    }

    fn increment_totals(&mut self, value: usize, amount: u32) {
        let low = amount as u16;
        if value >= self.last_bin_start {
            self.totals[15] = self.totals[15].wrapping_add(low);
            return;
        }
        let mut bin = value >> self.bin_shift;
        if bin & 1 != 0 {
            self.totals[bin] = self.totals[bin].wrapping_add(low);
            bin += 1;
        }
        for pair in (bin >> 1)..8 {
            let i = pair * 2;
            let old = self.totals[i] as u32 | (self.totals[i + 1] as u32) << 16;
            let new = old.wrapping_add(amount);
            self.totals[i] = new as u16;
            self.totals[i + 1] = (new >> 16) as u16;
        }
    }

    fn decrement_counts(&mut self, value: usize, amount: u32) {
        let neg = amount.wrapping_neg();
        let packed = (neg.wrapping_sub(1) << 16) | (neg & 0xFFFF);
        self.quick_increment(value, packed);
    }

    fn bin_of(&self, i: usize) -> usize {
        if i < self.last_bin_start {
            i >> self.bin_shift
        } else {
            15
        }
    }

    fn rescale(&mut self) {
        (self.bin_shift, self.last_bin_start) = best_shift(self.number + 1);
        let mut bins = [0i64; 16];
        self.counts[0] >>= 1;
        bins[self.bin_of(0)] += self.counts[0] as i64;
        let (mut max_count, mut max_pos) = (0u16, 0usize);
        let mut i = 1;
        let mut done = false;
        while i <= self.number && !done {
            while self.counts[i] <= 1 {
                if i < self.number {
                    self.counts[i] = self.counts[self.number];
                    self.values[i] = self.values[self.number];
                    self.counts[self.number] = 0;
                    self.number -= 1;
                } else {
                    self.counts[i] = 0;
                    self.number -= 1;
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
            self.counts[i] >>= 1;
            if self.counts[i] > max_count {
                max_count = self.counts[i];
                max_pos = i;
            }
            bins[self.bin_of(i)] += self.counts[i] as i64;
            i += 1;
        }
        if max_count > 0 {
            let mut swap = if self.number < self.last_bin_start { (self.number >> self.bin_shift) << self.bin_shift } else { self.last_bin_start };
            if swap == 0 {
                swap = 1;
            }
            if max_pos != swap {
                let old = self.counts[swap];
                self.counts[swap] = self.counts[max_pos];
                bins[self.bin_of(swap)] += -(old as i64) + self.counts[swap] as i64;
                bins[self.bin_of(max_pos)] += old as i64 - self.counts[swap] as i64;
                self.counts[max_pos] = old;
                self.values.swap(swap, max_pos);
            }
        }
        if self.number != self.unique as usize && self.counts[0] == 0 {
            self.counts[0] = self.counts[0].wrapping_add(2);
            bins[self.bin_of(0)] += 2;
        }
        let mut running = 0i64;
        for (t, b) in self.totals.iter_mut().zip(bins) {
            running += b;
            *t = running as u16;
        }
    }
}
