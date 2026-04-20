# ExecPlan

<proposed_plan>

## Summary
- Reproduced the current Rust-native workload on `large_sanitized_regen.json` at `2048x2048` with a fresh 3-run baseline: `prepare_total=0.3622s` mean.
- `arrangement` still edges out `classification` (`0.1470s` vs `0.1353s` mean), and detailed attribution shows `arrangement_pairs=0.0732s` as the largest arrangement subphase.
- Pair profiling shows the grid is admitting far more ordered candidates than real overlaps (`5.61M` ordered vs `1.16M` overlaps), so the next optimization should cut broad-phase false positives before touching finalize or classification.

## Key Changes
- Add a temporary internal override for pair-grid resolution so the same workload can be swept at multiple resolutions without changing behavior.
- Use that sweep to choose a tighter default candidate-pair grid heuristic that reduces broad-phase candidate count on the real fixture.
- Rebuild and re-measure the exact same release workload, accepting the change only if `prepare_total` improves and `arrangement_pairs` moves in the same direction.

## Public APIs / Internal Interfaces
- No public API changes.
- Internal changes stay within the arrangement broad-phase helpers in `rust/edge_drawer/src/lib.rs`.

## Test Plan
- `cargo test --manifest-path rust/edge_drawer/Cargo.toml`
- `cargo build --release --manifest-path rust/edge_drawer/Cargo.toml`
- Sweep the current workload with `EDGE_DRAWER_PAIR_GRID_RESOLUTION` on the same fixture to select a better resolution.
- Re-run `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json` three times.
- Run one attribution pass with `EDGE_DRAWER_ARRANGEMENT_PROFILE=1 EDGE_DRAWER_PAIR_PROFILE=1 EDGE_DRAWER_SPLIT_PROFILE=1`.

## Assumptions
- The current broad-phase grid clamp at `256` is still conservative for this workload and can be increased without pushing grid-build overhead above the saved pair-filtering cost.
- Candidate pair correctness depends only on coverage, not on the chosen resolution, so changing grid density preserves output semantics.
- The same fixture remains representative enough to validate the next improvement.

</proposed_plan>
