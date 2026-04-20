<proposed_plan>
Summary
- Continue the Rust-native optimization loop on the same `2048x2048` / `large_sanitized_regen.json` workload.
- Current attribution shows `arrangement_pairs` remains the dominant arrangement subphase at about `0.080s`, with `4.52M` ordered candidates and `2.05M` duplicate candidates.
- The current candidate-pair grid resolution is capped at `512`, which is below the natural next power-of-two target for this input size, so the first optimization is to increase grid granularity and re-measure duplicate suppression impact on the real workload.

Key Changes
- Raise the candidate-pair grid resolution cap so large segment sets can use a finer arrangement grid.
- Keep the rest of the pairing pipeline unchanged for the first iteration so the performance delta can be attributed to candidate generation density rather than unrelated logic changes.
- If the first iteration improves `prepare_total` or clearly lowers `arrangement_pairs`, re-profile and consider one more narrowly-scoped follow-up based on the new dominant cost.

Public APIs / Internal Interfaces
- No public API changes.
- Internal change only in the candidate-pair grid resolution heuristic inside `rust/edge_drawer/src/lib.rs`.

Test Plan
- `cargo build --release --manifest-path rust/edge_drawer/Cargo.toml`
- Baseline: 3 runs with `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`
- Attribution: 1 run with `EDGE_DRAWER_PROFILE=1 EDGE_DRAWER_ARRANGEMENT_PROFILE=1 EDGE_DRAWER_PAIR_PROFILE=1 ...`
- Re-run the same baseline and attribution commands after each accepted code change.

Assumptions
- Increasing the grid cap does not change output semantics because it only affects candidate enumeration, not geometric tests.
- Memory growth from a larger grid is acceptable for this workload in release mode.
- Acceptance is based on the primary metric `prepare_total`, with `arrangement_pairs` and `pair_stats` used as supporting evidence because this workload still shows visible run-to-run variance.
</proposed_plan>
