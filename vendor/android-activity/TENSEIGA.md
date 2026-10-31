android-activity 0.6.1 from crates.io, patched in (workspace `[patch.crates-io]`) for one
addition: `set_input_filter` (src/lib.rs), called from the NativeActivity input iterator
(src/native_activity/mod.rs), so the client can read game controllers before winit sees
their events. Nothing else differs from the published crate.
