# unrar_sys patch

unrar_sys **0.5.8**, vendored so `[patch.crates-io]` can replace the crates.io crate for Android without
editing `~/.cargo/registry`. Desktop and other platforms build the same vendored tree; only Android
targets take the changes below.

## Changes

1. `src/lib.rs` — `OpenArchiveDataEx::new` uses the byte-path constructor on Android (bionic), not
   the wide-character path used on Windows.
2. `vendor/unrar/ulinks.cpp` — bionic has no `lutimes`; Android builds call `utimes` on symlink
   targets, other Unix targets still use `lutimes`.