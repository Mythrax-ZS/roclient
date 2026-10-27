use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::table::{PacketLen, PacketTable};

#[derive(Debug)]
pub enum Frame {
    /// Header-less bytes, e.g. the bare 4-byte account_id the char server sends.
    Raw(BytesMut),
    /// Full packet including its opcode (and length field, if variable).
    Packet { opcode: u16, data: BytesMut },
}

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("unknown opcode {opcode:#06x}; next bytes: {preview:02x?}")]
    Unknown { opcode: u16, preview: Vec<u8> },
    #[error("bad variable length {0} for opcode {1:#06x}")]
    BadLen(usize, u16),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct RoCodec<T> {
    table: T,
    expect_raw: Option<usize>,
}

impl<T: PacketTable> RoCodec<T> {
    pub fn new(table: T) -> Self {
        Self { table, expect_raw: None }
    }

    /// The next `n` bytes are a header-less frame.
    pub fn expect_raw(&mut self, n: usize) {
        self.expect_raw = Some(n);
    }
}

impl<T: PacketTable> Decoder for RoCodec<T> {
    type Item = Frame;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, CodecError> {
        if let Some(n) = self.expect_raw {
            if src.len() < n {
                return Ok(None);
            }
            self.expect_raw = None;
            return Ok(Some(Frame::Raw(src.split_to(n))));
        }
        if src.len() < 2 {
            return Ok(None);
        }
        let opcode = u16::from_le_bytes([src[0], src[1]]);
        let len = match self.table.len_of(opcode) {
            Some(PacketLen::Fixed(n)) => n,
            Some(PacketLen::Variable) => {
                if src.len() < 4 {
                    return Ok(None);
                }
                let n = u16::from_le_bytes([src[2], src[3]]) as usize;
                if n < 4 {
                    return Err(CodecError::BadLen(n, opcode));
                }
                n
            }
            None => {
                let preview = src[..src.len().min(32)].to_vec();
                return Err(CodecError::Unknown { opcode, preview });
            }
        };
        if src.len() < len {
            src.reserve(len - src.len());
            return Ok(None);
        }
        Ok(Some(Frame::Packet { opcode, data: src.split_to(len) }))
    }
}

/// Outgoing packets serialize themselves, opcode included.
pub trait Encode {
    fn encode(&self, dst: &mut BytesMut);
}

impl<T: PacketTable, P: Encode> Encoder<P> for RoCodec<T> {
    type Error = CodecError;

    fn encode(&mut self, p: P, dst: &mut BytesMut) -> Result<(), CodecError> {
        p.encode(dst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::ServerTable;
    use bytes::BufMut;

    #[test]
    fn splits_fixed_variable_and_raw() {
        let mut c = RoCodec::new(ServerTable);
        let mut b = BytesMut::new();
        b.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]); // raw aid
        b.extend_from_slice(&[0x6c, 0x00, 0x05]); // HC_REFUSE_ENTER, fixed 3
        b.extend_from_slice(&[0x0d, 0x02, 0x04, 0x00]); // 0x20d, empty var packet
        c.expect_raw(4);
        assert!(matches!(c.decode(&mut b).unwrap(), Some(Frame::Raw(r)) if r.len() == 4));
        assert!(matches!(c.decode(&mut b).unwrap(), Some(Frame::Packet { opcode: 0x6c, .. })));
        assert!(matches!(c.decode(&mut b).unwrap(), Some(Frame::Packet { opcode: 0x20d, .. })));
        assert!(c.decode(&mut b).unwrap().is_none());
    }

    #[test]
    fn waits_for_partial() {
        let mut c = RoCodec::new(ServerTable);
        let mut b = BytesMut::from(&[0x6c, 0x00][..]);
        assert!(c.decode(&mut b).unwrap().is_none());
        b.put_u8(1);
        assert!(c.decode(&mut b).unwrap().is_some());
    }
}
