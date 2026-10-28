//! Minimal BSON reader for the client's contentdata\*.bson tables.

use anyhow::{bail, ensure, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Bson {
    Double(f64),
    Str(String),
    Doc(Vec<(String, Bson)>),
    Array(Vec<Bson>),
    Bool(bool),
    Int(i64),
    Null,
}

impl Bson {
    pub fn get(&self, key: &str) -> Option<&Bson> {
        match self {
            Bson::Doc(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Bson::Int(i) => Some(*i),
            Bson::Double(d) => Some(*d as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        if let Bson::Str(s) = self { Some(s) } else { None }
    }

    /// Children of a document or array.
    pub fn items(&self) -> Vec<&Bson> {
        match self {
            Bson::Doc(d) => d.iter().map(|(_, v)| v).collect(),
            Bson::Array(a) => a.iter().collect(),
            _ => vec![],
        }
    }
}

pub fn parse(data: &[u8]) -> Result<Bson> {
    let (doc, _) = document(data, 0)?;
    Ok(doc)
}

fn i32_at(d: &[u8], at: usize) -> Result<i32> {
    ensure!(at + 4 <= d.len(), "bson: truncated");
    Ok(i32::from_le_bytes(d[at..at + 4].try_into()?))
}

fn document(d: &[u8], at: usize) -> Result<(Bson, usize)> {
    let len = i32_at(d, at)? as usize;
    let end = at + len;
    ensure!(end <= d.len() && len >= 5, "bson: bad document length");
    let mut p = at + 4;
    let mut out = vec![];
    while p < end - 1 {
        let t = d[p];
        p += 1;
        let kend = p + d[p..end].iter().position(|c| *c == 0).ok_or_else(|| anyhow::anyhow!("bson: key"))?;
        let key = String::from_utf8_lossy(&d[p..kend]).into_owned();
        p = kend + 1;
        let v = match t {
            0x01 => {
                ensure!(p + 8 <= end, "bson: truncated");
                let v = f64::from_le_bytes(d[p..p + 8].try_into()?);
                p += 8;
                Bson::Double(v)
            }
            0x02 => {
                let n = i32_at(d, p)? as usize;
                ensure!(p + 4 + n <= end && n >= 1, "bson: string");
                let s = String::from_utf8_lossy(&d[p + 4..p + 4 + n - 1]).into_owned();
                p += 4 + n;
                Bson::Str(s)
            }
            0x03 | 0x04 => {
                let (v, next) = document(d, p)?;
                p = next;
                match (t, v) {
                    (0x04, Bson::Doc(items)) => Bson::Array(items.into_iter().map(|(_, v)| v).collect()),
                    (_, v) => v,
                }
            }
            0x08 => {
                p += 1;
                Bson::Bool(d[p - 1] != 0)
            }
            0x0a => Bson::Null,
            0x10 => {
                let v = i32_at(d, p)?;
                p += 4;
                Bson::Int(v as i64)
            }
            0x12 | 0x11 | 0x09 => {
                ensure!(p + 8 <= end, "bson: truncated");
                let v = i64::from_le_bytes(d[p..p + 8].try_into()?);
                p += 8;
                Bson::Int(v)
            }
            t => bail!("bson: unsupported type {t:#x}"),
        };
        out.push((key, v));
    }
    Ok((Bson::Doc(out), end))
}
