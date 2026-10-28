//! Reads constant tables out of compiled Lua 5.1 chunks (.lub) without running them.
//!
//! The client's data tables are flat assignments like
//!   jobtbl = { JT_PORING = 1002, ... }                       (npcidentity)
//!   JobNameTable = { [jobtbl.JT_PORING] = "PORING", ... }   (jobname)
//! which compile to LOADK / GETTABLE / SETTABLE sequences over the constant pool.
//! Tracking those three opcodes recovers every (key, value) pair.

use anyhow::{bail, ensure, Result};
use encoding_rs::EUC_KR;

use crate::reader::Reader;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
    /// A table built by the chunk (index into `Chunk::tables`).
    Table(usize),
    /// An undefined global read by name.
    Global(String),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }

    pub fn as_number(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
}

struct Function {
    code: Vec<u32>,
    constants: Vec<Value>,
}

fn string(r: &mut Reader) -> Result<String> {
    let len = r.u32()? as usize;
    if len == 0 {
        return Ok(String::new());
    }
    let bytes = r.bytes(len)?;
    // Includes the trailing NUL. Names are ASCII; values may be EUC-KR.
    Ok(EUC_KR.decode(&bytes[..len - 1]).0.into_owned())
}

fn function(r: &mut Reader, depth: usize) -> Result<Function> {
    ensure!(depth < 64, "lub: nesting too deep");
    string(r)?; // source
    r.bytes(8)?; // linedefined, lastlinedefined
    r.bytes(4)?; // nups, numparams, is_vararg, maxstacksize
    let n = r.u32()? as usize;
    let code = (0..n).map(|_| r.u32()).collect::<Result<_>>()?;
    let n = r.u32()? as usize;
    let mut constants = Vec::with_capacity(n);
    for _ in 0..n {
        constants.push(match r.u8()? {
            0 => Value::Nil,
            1 => Value::Bool(r.u8()? != 0),
            3 => Value::Number(f64::from_le_bytes(r.bytes(8)?.try_into()?)),
            4 => Value::Str(string(r)?),
            t => bail!("lub: unknown constant type {t}"),
        });
    }
    let protos = r.u32()? as usize;
    for _ in 0..protos {
        function(r, depth + 1)?;
    }
    // Debug info: line info, locals, upvalue names.
    let n = r.u32()? as usize;
    r.bytes(n * 4)?;
    let n = r.u32()? as usize;
    for _ in 0..n {
        string(r)?;
        r.bytes(8)?;
    }
    let n = r.u32()? as usize;
    for _ in 0..n {
        string(r)?;
    }
    Ok(Function { code, constants })
}

/// Every `table[key] = value` assignment in the chunk's main function whose key and
/// value resolve to constants. A key read as `tbl.NAME` is reported as `NAME`.
pub fn assignments(data: &[u8]) -> Result<Vec<(Value, Value)>> {
    let mut r = Reader::new(data);
    ensure!(r.bytes(4)? == b"\x1bLua", "not a Lua chunk");
    ensure!(r.u8()? == 0x51, "only Lua 5.1 chunks are supported");
    let header = r.bytes(7)?;
    ensure!(header == [0, 1, 4, 4, 4, 8, 0], "unsupported Lua 5.1 build {header:?}");
    let f = function(&mut r, 0)?;

    const GETTABLE: u32 = 6;
    const LOADK: u32 = 1;
    const SETTABLE: u32 = 9;
    let mut regs: Vec<Value> = vec![Value::Nil; 256];
    let rk = |regs: &[Value], x: u32| -> Value {
        if x & 0x100 != 0 { f.constants.get((x & 0xff) as usize).cloned().unwrap_or(Value::Nil) } else { regs[x as usize].clone() }
    };
    let mut out = vec![];
    for &ins in &f.code {
        let op = ins & 0x3f;
        let a = ((ins >> 6) & 0xff) as usize;
        let c = (ins >> 14) & 0x1ff;
        let b = ins >> 23;
        match op {
            LOADK => regs[a] = f.constants.get((ins >> 14) as usize).cloned().unwrap_or(Value::Nil),
            // `tbl.NAME`: remember the field name; the table itself does not matter here.
            GETTABLE => regs[a] = rk(&regs, c),
            SETTABLE => {
                let (k, v) = (rk(&regs, b), rk(&regs, c));
                if k != Value::Nil && v != Value::Nil {
                    out.push((k, v));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Result of running a data chunk's main function: its globals and the tables it built.
pub struct Chunk {
    pub globals: std::collections::HashMap<String, Value>,
    pub tables: Vec<Vec<(Value, Value)>>,
}

impl Chunk {
    /// Entries of a table value (empty when `v` is not a table).
    pub fn entries(&self, v: &Value) -> &[(Value, Value)] {
        match v {
            Value::Table(i) => self.tables.get(*i).map_or(&[], Vec::as_slice),
            _ => &[],
        }
    }

    /// `t[key]` for a string key.
    pub fn field<'a>(&'a self, t: &Value, key: &str) -> Option<&'a Value> {
        self.entries(t).iter().find(|(k, _)| k.as_str() == Some(key)).map(|(_, v)| v)
    }
}

/// Evaluates the straight-line data-building code in a chunk's main function:
/// constants, globals, nested table constructors (NEWTABLE / SETTABLE / SETLIST).
/// Function calls and control flow are ignored, which data tables do not use.
pub fn eval(data: &[u8]) -> Result<Chunk> {
    let mut r = Reader::new(data);
    ensure!(r.bytes(4)? == b"\x1bLua", "not a Lua chunk");
    ensure!(r.u8()? == 0x51, "only Lua 5.1 chunks are supported");
    let header = r.bytes(7)?;
    ensure!(header == [0, 1, 4, 4, 4, 8, 0], "unsupported Lua 5.1 build {header:?}");
    let f = function(&mut r, 0)?;

    const MOVE: u32 = 0;
    const LOADK: u32 = 1;
    const LOADBOOL: u32 = 2;
    const LOADNIL: u32 = 3;
    const GETGLOBAL: u32 = 5;
    const GETTABLE: u32 = 6;
    const SETGLOBAL: u32 = 7;
    const SETTABLE: u32 = 9;
    const NEWTABLE: u32 = 10;
    const SETLIST: u32 = 34;
    const FIELDS_PER_FLUSH: u32 = 50;

    let mut chunk = Chunk { globals: Default::default(), tables: vec![] };
    let mut regs: Vec<Value> = vec![Value::Nil; 256];
    let k = |i: u32| f.constants.get(i as usize).cloned().unwrap_or(Value::Nil);
    let rk = |regs: &[Value], x: u32| if x & 0x100 != 0 { k(x & 0xff) } else { regs[x as usize].clone() };
    let mut pc = 0;
    while pc < f.code.len() {
        let ins = f.code[pc];
        pc += 1;
        let op = ins & 0x3f;
        let a = ((ins >> 6) & 0xff) as usize;
        let c = (ins >> 14) & 0x1ff;
        let b = ins >> 23;
        let bx = ins >> 14;
        match op {
            MOVE => regs[a] = regs[b as usize].clone(),
            LOADK => regs[a] = k(bx),
            LOADBOOL => regs[a] = Value::Bool(b != 0),
            LOADNIL => regs[a..=(b as usize).min(255)].fill(Value::Nil),
            GETGLOBAL => {
                let name = k(bx).as_str().unwrap_or_default().to_string();
                regs[a] = chunk.globals.get(&name).cloned().unwrap_or(Value::Global(name));
            }
            SETGLOBAL => {
                if let Some(name) = k(bx).as_str() {
                    chunk.globals.insert(name.to_string(), regs[a].clone());
                }
            }
            NEWTABLE => {
                chunk.tables.push(vec![]);
                regs[a] = Value::Table(chunk.tables.len() - 1);
            }
            GETTABLE => {
                let key = rk(&regs, c);
                regs[a] = match &regs[b as usize] {
                    Value::Table(t) => chunk.tables[*t].iter().rev().find(|(kk, _)| *kk == key).map_or(Value::Nil, |(_, v)| v.clone()),
                    // `undefinedtable.NAME`: keep the field name (jobname's `jobtbl.JT_X` keys).
                    _ => key,
                };
            }
            SETTABLE => {
                if let Value::Table(t) = regs[a] {
                    let (key, v) = (rk(&regs, b), rk(&regs, c));
                    if key != Value::Nil {
                        chunk.tables[t].push((key, v));
                    }
                }
            }
            SETLIST => {
                let block = if c == 0 {
                    pc += 1;
                    f.code.get(pc - 1).copied().unwrap_or(1)
                } else {
                    c
                };
                if let Value::Table(t) = regs[a] {
                    for i in 1..=b {
                        let idx = (block - 1) * FIELDS_PER_FLUSH + i;
                        let v = regs.get(a + i as usize).cloned().unwrap_or(Value::Nil);
                        chunk.tables[t].push((Value::Number(idx as f64), v));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_samples_if_present() {
        let Ok(dir) = std::env::var("RO_LUB_DIR") else { return };
        let ids = assignments(&std::fs::read(format!("{dir}/npcidentity.lub")).unwrap()).unwrap();
        assert!(ids.iter().any(|(k, v)| k.as_str() == Some("JT_PORING") && v.as_number() == Some(1002.0)));
        let names = assignments(&std::fs::read(format!("{dir}/jobname.lub")).unwrap()).unwrap();
        assert!(names.iter().any(|(k, v)| k.as_str() == Some("JT_PORING") && v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("poring"))));
    }
}

#[cfg(test)]
mod eval_tests {
    use super::*;

    #[test]
    fn iteminfo_if_present() {
        let Ok(path) = std::env::var("RO_ITEMINFO") else { return };
        let c = eval(&std::fs::read(path).unwrap()).unwrap();
        let tbl = c.globals.get("tbl").expect("tbl global");
        let red = c.entries(tbl).iter().find(|(k, _)| k.as_number() == Some(501.0)).map(|(_, v)| v).unwrap();
        assert_eq!(c.field(red, "identifiedDisplayName").and_then(Value::as_str), Some("Red Potion"));
        let desc = c.field(red, "identifiedDescriptionName").unwrap();
        assert!(c.entries(desc).len() >= 3);
    }
}

/// Rewrites a Lua 5.1 chunk compiled with a 4-byte size_t (the client's 32-bit build) for
/// a 64-bit Lua 5.1, which reads string lengths as 8 bytes. Returns other chunks as-is.
pub fn widen_size_t(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.len() >= 12 && &data[..4] == b"\x1bLua" && data[4] == 0x51, "not a Lua 5.1 chunk");
    if data[8] == 8 {
        return Ok(data.to_vec());
    }
    ensure!(data[6] == 1 && data[7] == 4 && data[8] == 4 && data[9] == 4 && data[10] == 8, "unsupported Lua 5.1 build {:?}", &data[5..12]);
    let mut r = Reader::new(&data[12..]);
    let mut out = data[..12].to_vec();
    out[8] = 8;
    fn string(r: &mut Reader, out: &mut Vec<u8>) -> Result<()> {
        let n = r.u32()? as usize;
        out.extend((n as u64).to_le_bytes());
        out.extend(r.bytes(n)?);
        Ok(())
    }
    fn ints(r: &mut Reader, out: &mut Vec<u8>, each: usize) -> Result<usize> {
        let n = r.u32()? as usize;
        out.extend((n as u32).to_le_bytes());
        out.extend(r.bytes(n * each)?);
        Ok(n)
    }
    fn function(r: &mut Reader, out: &mut Vec<u8>, depth: u32) -> Result<()> {
        ensure!(depth < 200, "Lua chunk nests too deeply");
        string(r, out)?; // source
        out.extend(r.bytes(4 + 4 + 4)?); // line defined, last line, nups/params/vararg/maxstack
        ints(r, out, 4)?; // code
        let k = r.u32()?;
        out.extend(k.to_le_bytes());
        for _ in 0..k {
            let t = r.u8()?;
            out.push(t);
            match t {
                0 => {}
                1 => out.push(r.u8()?),
                3 => out.extend(r.bytes(8)?),
                4 => string(r, out)?,
                t => bail!("bad constant type {t}"),
            }
        }
        let p = r.u32()?;
        out.extend(p.to_le_bytes());
        for _ in 0..p {
            function(r, out, depth + 1)?;
        }
        ints(r, out, 4)?; // line info
        let locals = r.u32()?;
        out.extend(locals.to_le_bytes());
        for _ in 0..locals {
            string(r, out)?;
            out.extend(r.bytes(8)?);
        }
        let ups = r.u32()?;
        out.extend(ups.to_le_bytes());
        for _ in 0..ups {
            string(r, out)?;
        }
        Ok(())
    }
    function(&mut r, &mut out, 0)?;
    Ok(out)
}
