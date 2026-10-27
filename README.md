# roclient

A from-scratch Ragnarok Online client for rAthena, in Rust (wgpu: Vulkan / DX12 / GL).

Status: login → character select → map entry → stay connected (headless). Rendering is a
window that clears; map and sprite loading come next.

## Target

PACKETVER **20250402** (main), the newest rAthena supports. The server must be built to match.
Add these lines to rAthena's `src/custom/defines_pre.hpp`:

```cpp
#define PACKETVER 20250402
#undef PACKET_OBFUSCATION   // not implemented in the client yet
```

and set `pincode_enabled: no` in `conf/char_athena.conf`.

## Crates

| crate | role |
|---|---|
| `ro-proto` | framing codec, packet encode/parse, generated packet table |
| `ro-net` | login / char / map connection state machines |
| `ro-vfs` | GRF 0x200 + loose `data/` folder |
| `ro-render` | wgpu renderer |
| `ro-client` | binary |
| `tools/packetgen` | regenerates `ro-proto/src/generated.rs` from rAthena |
| `tools/grfdump` | list / extract GRF contents |

## Run

```
copy config\example.toml config\local.toml    # edit account
cargo run -p ro-client -- --headless          # protocol smoke test
cargo run -p ro-client                        # window
```

`RUST_LOG=debug` shows every packet that was skipped.

## Packet table

`crates/ro-proto/src/generated.rs` is checked in. To regenerate it for another rAthena revision
or PACKETVER (requires Visual Studio with the C++ workload):

```
cargo run -p packetgen -- ..\rathena 20250402
```

packetgen compiles a small program against rAthena's real headers (`map/packets.hpp`,
`common/packets.hpp`, `clif_packetdb.hpp`, `clif_shuffle.hpp`) with MSVC. That way every
length, opcode and field offset is the one the server itself uses, with no re-implemented
`#if PACKETVER` logic. Client packets are encoded from the handler's most recent
registration (`client_packet("WalkToXY")`), so shuffled opcodes follow the table.
