use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
};

use anyhow::{bail, ensure, Context};
use encoding_rs::EUC_KR;
use flate2::read::ZlibDecoder;

const HEADER_LEN: u64 = 46;
const FLAG_FILE: u8 = 0x01;
const FLAG_MIXED_CRYPT: u8 = 0x02;
const FLAG_HEADER_CRYPT: u8 = 0x04;

struct Entry {
    name: String,
    comp_len: u32,
    aligned_len: u32,
    real_len: u32,
    flags: u8,
    offset: u64,
}

/// GRF 0x200 archive. Older encrypted (0x102/0x103) archives are not supported yet.
pub struct Grf {
    file: Mutex<File>,
    entries: HashMap<String, Entry>,
}

impl Grf {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut h = [0u8; HEADER_LEN as usize];
        f.read_exact(&mut h)?;
        // Newer official clients replace the magic with "Event Horizon"; the layout is unchanged.
        ensure!(
            h.starts_with(b"Master of Magic") || h.starts_with(b"Event Horizon\0"),
            "{} is not a GRF",
            path.display()
        );
        let rd = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
        let version = rd(42);
        // 0x200: u32 table offset, seed, count + seed + 7.
        // 0x300: u64 table offset, plain count; the table gains a u32 prefix and entries a u64 offset.
        let (table_off, count, wide) = match version {
            0x200 => (rd(30) as u64, rd(38).wrapping_sub(rd(34)).wrapping_sub(7), false),
            0x300 => (u64::from_le_bytes(h[30..38].try_into()?), rd(38), true),
            v => bail!("GRF version {v:#x} unsupported (0x200 and 0x300 only)"),
        };

        f.seek(SeekFrom::Start(HEADER_LEN + table_off + if wide { 4 } else { 0 }))?;
        let mut sz = [0u8; 8];
        f.read_exact(&mut sz)?;
        let comp = u32::from_le_bytes(sz[0..4].try_into()?) as usize;
        let real = u32::from_le_bytes(sz[4..8].try_into()?) as usize;
        let mut cbuf = vec![0; comp];
        f.read_exact(&mut cbuf)?;
        let mut table = Vec::with_capacity(real);
        ZlibDecoder::new(&cbuf[..]).read_to_end(&mut table)?;

        let mut entries = HashMap::with_capacity(count as usize);
        let mut i = 0;
        while i < table.len() {
            let nul = table[i..].iter().position(|&b| b == 0).context("truncated file table")?;
            let end = i + nul;
            let meta = if wide { 21 } else { 17 };
            ensure!(end + 1 + meta <= table.len(), "truncated file table");
            let (name, _, _) = EUC_KR.decode(&table[i..end]);
            let m = &table[end + 1..end + 1 + meta];
            let r = |o: usize| u32::from_le_bytes(m[o..o + 4].try_into().unwrap());
            let offset = if wide { u64::from_le_bytes(m[13..21].try_into().unwrap()) } else { r(13) as u64 };
            let e = Entry { name: name.into_owned(), comp_len: r(0), aligned_len: r(4), real_len: r(8), flags: m[12], offset };
            entries.insert(crate::normalize(&e.name), e);
            i = end + 1 + meta;
        }
        tracing::info!(path = %path.display(), files = entries.len(), "opened GRF");
        Ok(Self { file: Mutex::new(f), entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Original (decoded) file names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.values().filter(|e| e.flags & FLAG_FILE != 0).map(|e| e.name.as_str())
    }

    pub fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(e) = self.entries.get(&crate::normalize(path)) else {
            return Ok(None);
        };
        if e.flags & FLAG_FILE == 0 {
            return Ok(None);
        }
        let mut buf = vec![0; e.aligned_len as usize];
        {
            let mut f = self.file.lock().unwrap();
            f.seek(SeekFrom::Start(HEADER_LEN + e.offset))?;
            f.read_exact(&mut buf)?;
        }
        if e.flags & FLAG_MIXED_CRYPT != 0 {
            crate::des::decode_mixed(&mut buf, e.comp_len);
        } else if e.flags & FLAG_HEADER_CRYPT != 0 {
            crate::des::decode_header(&mut buf);
        }
        buf.truncate(e.comp_len as usize);
        let mut out = Vec::with_capacity(e.real_len as usize);
        ZlibDecoder::new(&buf[..]).read_to_end(&mut out)?;
        Ok(Some(out))
    }
}
