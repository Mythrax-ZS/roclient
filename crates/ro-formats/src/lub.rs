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
