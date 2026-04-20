# Progress

- 2026-04-20 10:33 JST: Created run directory `rust_native_20260420_103338`.
- 2026-04-20 10:34 JST: Rebuilt `rust/edge_drawer` in release mode.
- 2026-04-20 10:34 JST: Ran fresh 3-run baseline on `.agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`.
- 2026-04-20 10:34 JST: Attribution confirmed `arrangement_pairs=0.0732s` with `ordered_candidates=5.61M` and `overlaps=1.16M`.
- 2026-04-20 10:35 JST: Chose pair-grid tuning as the next optimization target.
- 2026-04-20 10:36 JST: Added an env-gated pair-grid resolution override and swept `32/64/128/256/512/1024` on the same workload.
- 2026-04-20 10:38 JST: Single-sample sweep suggested `128` could help, but a fixed 3-run validation regressed `prepare_total`, so the grid-tuning idea was rejected.
- 2026-04-20 10:40 JST: Re-profiled classification and confirmed the hot path still does two independent polygon-grid candidate walks per segment side test.
- 2026-04-20 10:44 JST: Implemented a shared-candidate fast path for `segment_side_states_with_polygons()` when the left/right sample points land in the same polygon-grid cell.
- 2026-04-20 10:45 JST: `cargo test --manifest-path rust/edge_drawer/Cargo.toml` passed (31 tests). Debug build emitted an incremental cleanup permission warning, but tests completed successfully.
- 2026-04-20 10:47 JST: First 3-run post-change validation improved `prepare_total` from `0.3622s` mean to `0.3546s` mean (`-2.11%`).
- 2026-04-20 10:48 JST: A second 3-run validation sample kept the win direction and brought the combined post-change mean to `0.3526s` (`-2.66%` vs baseline).
