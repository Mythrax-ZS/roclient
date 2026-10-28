use anyhow::{ensure, Result};
use encoding_rs::EUC_KR;

/// Little-endian cursor with bounds checks.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.pos + n <= self.data.len(), "unexpected end of data at {} (+{n})", self.pos);
        let b = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(b)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into()?))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into()?))
    }

    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into()?))
    }

    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into()?))
    }

    /// Fixed-size, NUL-padded EUC-KR string.
    pub fn string(&mut self, n: usize) -> Result<String> {
        let raw = self.bytes(n)?;
        let end = raw.iter().position(|&c| c == 0).unwrap_or(n);
        Ok(EUC_KR.decode(&raw[..end]).0.into_owned())
    }
}
