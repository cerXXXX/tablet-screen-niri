# Vendored: moreland's capture and encoder crates

`mbuf_capture` and `mbuf_encoder` are `crates/capture` and `crates/encoder` from
[moreland](https://github.com/adiimanav/moreland) (Apache-2.0, SPDX headers
kept). They are vendored rather than depended on by path/git so that this
repository builds on its own and a package can be produced from it.

What was changed:

* `crates/*/Cargo.toml` — workspace inheritance (`version.workspace = true`
  and friends) expanded to concrete values, the probe `[[bin]]` targets and
  their sources removed.
* `capture/src/session.rs` — `pick_dmabuf_format()` now prefers **AR24** over
  XR24. Intel's `vapostproc` lists AR24 in its DMABUF caps and XR24 not at all,
  so asking for XR24 yields a modifier the encoder cannot import (or LINEAR,
  which it also refuses) and the pipeline fails with `not-negotiated`.
* `capture/src/lib.rs` — `AR24` constant, with the same reasoning.

Verified on this machine: 2400x1500, Intel Y-tiled (0x0100000000000002),
59 fps, ~8% of one core for capture+encode.
