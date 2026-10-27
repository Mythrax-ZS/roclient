//! packetgen: derive the packet table for a PACKETVER from rAthena's own headers.
//!
//!   packetgen <rathena-dir> [packetver] [out.rs]
//!
//! Rather than re-implementing rAthena's `#if PACKETVER` logic, this compiles a small
//! C++ program against the real headers with MSVC so that every length is whatever
//! the server itself uses:
//!   1. preprocess map/packets.hpp + common/packets.hpp to learn which PACKET_* structs
//!      and HEADER_* ids survive at this version, and which are variable-length;
//!   2. compile a dumper that prints sizeof() of those structs plus every entry of
//!      clif_packetdb.hpp / clif_shuffle.hpp;
//!   3. emit crates/ro-proto/src/generated.rs.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, ensure, Context};
use regex::Regex;

const SERVER_PREFIXES: &[&str] = &["ZC_", "HC_", "AC_", "SC_"];
const CLIENT_PREFIXES: &[&str] = &["CZ_", "CH_", "CA_"];

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rathena = PathBuf::from(args.first().context("usage: packetgen <rathena-dir> [packetver] [out.rs]")?);
    let packetver: u32 = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(20250402);
    let out = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/ro-proto/src/generated.rs"));

    let work = std::env::temp_dir().join(format!("roclient-packetgen-{packetver}"));
    std::fs::create_dir_all(&work)?;
    let src = absolute(&rathena.join("src"))?;
    let third = absolute(&rathena.join("3rdparty"))?;
    let includes = [
        src.clone(),
        third.join("rapidyaml/src"),
        third.join("rapidyaml/ext/c4core/src"),
        third.join("pcre/include"),
        third.join("libconfig"),
        third.join("mysql/include"),
    ];
    let mut flags = vec![
        "/nologo".to_string(),
        "/std:c++17".into(),
        "/EHsc".into(),
        "/W0".into(),
        "/utf-8".into(),
        format!("/DPACKETVER={packetver}"),
        "/DWIN32".into(),
        "/DFD_SETSIZE=4096".into(),
        "/D_CRT_SECURE_NO_DEPRECATE".into(),
        "/D_WINSOCK_DEPRECATED_NO_WARNINGS".into(),
        "/DLIBCONFIG_STATIC".into(),
        "/DNDEBUG".into(),
    ];
    flags.extend(includes.iter().map(|p| format!("/I{}", p.display())));

    // 1. which structs exist at this version
    // Both headers use the include guard PACKETS_HPP and share a few struct names, so the
    // char/login one goes into namespace `cpk`. Its dependencies are included first so
    // their own guards keep them out of the namespace.
    let headers = r#"#include <functional>
#include <unordered_map>
#include "common/cbasetypes.hpp"
#include "common/mmo.hpp"
#include "common/showmsg.hpp"
#include "common/socket.hpp"
#include "common/utilities.hpp"
namespace cpk {
#include "common/packets.hpp"
}
#undef PACKETS_HPP
#undef DEFINE_PACKET_HEADER
#include "map/packets.hpp"
"#;
    let mut structs: BTreeMap<String, (bool, &str)> = BTreeMap::new();
    for (header, ns) in [("map/packets.hpp", ""), ("common/packets.hpp", "cpk::")] {
        std::fs::write(work.join("pp.cpp"), format!("#include \"{header}\"\n"))?;
        let pp = run_cl(&work, &[&flags[..], &["/EP".into(), "pp.cpp".into()]].concat())?;
        for (name, var) in scan_structs(&pp) {
            structs.entry(name).or_insert((var, ns));
        }
    }
    eprintln!("packetgen: {} packet structs with headers at PACKETVER {packetver}", structs.len());

    // 2. dump sizes and packet db
    let mut dump = String::from("#include <cstdio>\n");
    dump += headers;
    dump += r#"
static void rec(int cmd, int len, const char* func, const char* pos) {
    std::printf("db %d %d %s %s\n", cmd, len, func, pos);
}
#define packetdb_addpacket(cmd, len, func, ...) rec((int)(cmd), (int)(int16)(len), #func, "[" #__VA_ARGS__ "]")
int main() {
#include "map/clif_packetdb.hpp"
#include "map/clif_shuffle.hpp"
"#;
    for (name, (_, ns)) in &structs {
        writeln!(dump, "    std::printf(\"st {name} %d %d\\n\", (int)(uint16){ns}HEADER_{name}, (int)sizeof({ns}PACKET_{name}));")?;
    }
    dump += "    return 0;\n}\n";
    std::fs::write(work.join("dump.cpp"), dump)?;
    run_cl(&work, &[&flags[..], &["dump.cpp".into(), "/Fe:dump.exe".into()]].concat())?;
    let output = Command::new(work.join("dump.exe")).output()?;
    ensure!(output.status.success(), "dump.exe failed");
    let text = String::from_utf8(output.stdout)?;

    // 3. merge
    let mut server: BTreeMap<u16, (i32, String)> = BTreeMap::new();
    // opcode -> (len, handler, field offsets, registration order)
    let mut client: BTreeMap<u16, (i32, String, Vec<u16>, usize)> = BTreeMap::new();
    let mut seq = 0usize;
    let mut names: BTreeMap<String, u16> = BTreeMap::new();
    for line in text.lines() {
        let mut it = line.splitn(5, ' ');
        match it.next() {
            Some("db") => {
                let cmd: i32 = it.next().unwrap().parse()?;
                let len: i32 = it.next().unwrap().parse()?;
                let func = it.next().unwrap();
                let pos = it.next().unwrap_or("[]");
                let cmd = cmd as u16;
                if func == "nullptr" {
                    server.insert(cmd, (len, String::new()));
                } else {
                    let mut pos: Vec<u16> = pos
                        .trim_matches(|c| c == '[' || c == ']')
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();
                    pos.pop(); // trailing 0 terminator
                    let func = func.trim_start_matches("clif_parse_").to_string();
                    seq += 1;
                    client.insert(cmd, (len, func, pos, seq));
                }
            }
            Some("st") => {
                let name = it.next().unwrap().to_string();
                let id: i32 = it.next().unwrap().parse()?;
                let size: i32 = it.next().unwrap().parse()?;
                let id = id as u16;
                let len = if structs[&name].0 { -1 } else { size };
                if SERVER_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    if let Some((old, _)) = server.get(&id) {
                        if *old != len {
                            eprintln!("packetgen: note {id:#06x} {name}: packet db says {old}, struct says {len} (using struct)");
                        }
                    }
                    server.insert(id, (len, name.clone()));
                } else if CLIENT_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    client.entry(id).or_insert_with(|| (len, String::new(), vec![], 0));
                }
                names.insert(name, id);
            }
            _ => {}
        }
    }
    ensure!(!server.is_empty() && !client.is_empty(), "dumper produced no packets");

    let commit = Command::new("git")
        .arg("-C")
        .arg(&rathena)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let mut rs = String::new();
    writeln!(rs, "// @generated by tools/packetgen from rathena {commit} at PACKETVER {packetver}. Do not edit.")?;
    writeln!(rs, "#![allow(non_upper_case_globals, dead_code)]\n")?;
    writeln!(rs, "use super::table::ClientPacket;\n")?;
    writeln!(rs, "pub const PACKETVER: u32 = {packetver};\n")?;
    writeln!(rs, "/// Server -> client: (opcode, length); -1 = variable.")?;
    writeln!(rs, "pub static SERVER: &[(u16, i32)] = &[")?;
    for (id, (len, name)) in &server {
        let c = if name.is_empty() { String::new() } else { format!(" // {name}") };
        writeln!(rs, "    ({id:#06x}, {len}),{c}")?;
    }
    writeln!(rs, "];\n")?;
    writeln!(rs, "/// Client -> server, as the map server parses them. A handler may be reachable through")?;
    writeln!(rs, "/// several opcodes; `seq` is registration order, and the highest is the current one.")?;
    writeln!(rs, "pub static CLIENT: &[ClientPacket] = &[")?;
    for (id, (len, func, pos, seq)) in &client {
        writeln!(rs, "    ClientPacket {{ opcode: {id:#06x}, len: {len}, handler: {func:?}, pos: &{pos:?}, seq: {seq} }},")?;
    }
    writeln!(rs, "];\n")?;
    writeln!(rs, "/// Opcodes of every struct-defined packet at this version.")?;
    writeln!(rs, "pub mod id {{")?;
    for (name, id) in &names {
        writeln!(rs, "    pub const {name}: u16 = {id:#06x};")?;
    }
    writeln!(rs, "}}")?;
    std::fs::write(&out, rs)?;
    eprintln!("packetgen: wrote {} server / {} client packets to {}", server.len(), client.len(), out.display());
    Ok(())
}

/// Returns struct name -> is_variable for every PACKET_X that also has a HEADER_X.
fn scan_structs(pp: &str) -> BTreeMap<String, bool> {
    let header = Regex::new(r"const\s+int16\s+HEADER_(\w+)\s*=").unwrap();
    let headers: BTreeSet<&str> = header.captures_iter(pp).map(|c| c.get(1).unwrap().as_str()).collect();
    let st = Regex::new(r"struct\s+PACKET_(\w+)\s*\{([^}]*)\}").unwrap();
    let mut out = BTreeMap::new();
    for c in st.captures_iter(pp) {
        let name = &c[1];
        if !headers.contains(name) {
            continue;
        }
        let body = &c[2];
        let mut fields = body.split(';').map(str::trim).filter(|s| !s.is_empty());
        let _ty = fields.next();
        let second = fields.next().unwrap_or("");
        let var = second.ends_with("ength") || second.ends_with("Len") || second.ends_with(" len") || body.contains("[]");
        out.insert(name.to_string(), var);
    }
    out
}

/// canonicalize() without the `\\?\` prefix cl.exe rejects.
fn absolute(p: &Path) -> anyhow::Result<PathBuf> {
    let c = p.canonicalize().with_context(|| format!("{} not found", p.display()))?;
    let s = c.to_string_lossy();
    Ok(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s)))
}

fn vcvars() -> anyhow::Result<PathBuf> {
    let vswhere = Path::new(r"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe");
    let o = Command::new(vswhere)
        .args(["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"])
        .output()
        .context("vswhere not found; install Visual Studio with the C++ workload")?;
    let root = String::from_utf8(o.stdout)?.trim().to_string();
    ensure!(!root.is_empty(), "no Visual Studio with C++ tools found");
    Ok(Path::new(&root).join(r"VC\Auxiliary\Build\vcvars64.bat"))
}

/// Runs cl.exe inside a vcvars environment; returns stdout.
fn run_cl(work: &Path, args: &[String]) -> anyhow::Result<String> {
    let bat = work.join("cl.bat");
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
    std::fs::write(&bat, format!("@call \"{}\" >nul\r\n@cl {}\r\n", vcvars()?.display(), quoted.join(" ")))?;
    let o = Command::new("cmd").arg("/C").arg(&bat).current_dir(work).output()?;
    if !o.status.success() {
        let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        let errors: Vec<&str> = out.lines().filter(|l| l.contains("error")).take(20).collect();
        bail!("cl failed:\n{}", errors.join("\n"));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}
