Summary
- Reproduced the current Rust-native workload on `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`.
- Fresh 3-run baseline on the current head is `prepare_total=0.5678s` mean and `arrangement=0.2892s` mean.
- Detail profiling shows `arrangement_finalize=0.1232s` versus `arrangement_pairs=0.0803s`, so this pass targets finalize-side lookup and reconstruction overhead instead of pair enumeration.

Key Changes
- Replace finalize-time `HashMap<CanonicalSegment, Vec<CanonicalSegment>>` lookups with index-addressed storage keyed by `original_segments`.
- Build a one-time `CanonicalSegment -> index` map only for group remapping after splits, then reuse indexed access for group reconstruction and changed-group detection.
- Re-measure the same workload and accept the change only if `prepare_total` improves on the same fixed benchmark, with `arrangement` or `arrangement_finalize` moving first.

Public APIs / Internal Interfaces
- No public API changes.
- Internal Rust-only changes in `build_segment_arrangement_from_parts()`.
- Existing benchmark command and profiling keys remain unchanged.

Test Plan
- `cargo build --release --manifest-path rust/edge_drawer/Cargo.toml`
- `cargo test --manifest-path rust/edge_drawer/Cargo.toml test_arrangement_ -- --nocapture`
- Re-run the same native workload 3 times with `EDGE_DRAWER_PROFILE=1`
- Re-run one attribution sample with `EDGE_DRAWER_PROFILE=1 EDGE_DRAWER_PAIR_PROFILE=1 EDGE_DRAWER_SPLIT_PROFILE=1 EDGE_DRAWER_ARRANGEMENT_PROFILE=1`

Assumptions
- The fixed `large_sanitized_regen.json` workload is still representative of arrangement-heavy real usage.
- `original_segments` remains unique and stable, so index-addressed finalize storage is behavior-preserving.
- A measurable win should appear in `arrangement_finalize` before any broader end-to-end gain is accepted.
