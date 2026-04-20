# Result
- Workload: `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`
- Accepted change:
  - reuse the same polygon-grid candidate slice for both side-sample queries in `segment_side_states_with_polygons()` whenever both sample points fall in the same dense-grid cell
- Rejected change:
  - lowering the default pair-grid resolution after a sweep-driven experiment; the follow-up 3-run validation regressed end-to-end time
- Primary metric `prepare_total`:
  - baseline mean over 3 runs: `0.3622s`
  - accepted validation mean over 3 runs: `0.3546s` (`-0.0076s`, `-2.11%`)
  - combined post-change mean over 6 runs: `0.3526s` (`-0.0097s`, `-2.66%`)
- Supporting metrics, combined post-change mean vs baseline:
  - `classification`: `0.1353s` -> `0.1265s` (`-0.0088s`, `-6.53%`)
  - `path_build`: `0.0761s` -> `0.0738s` (`-0.0023s`, `-3.00%`)
  - `arrangement`: `0.1470s` -> `0.1485s` (`+0.0015s`, `+1.00%`)
  - `render_raster`: `0.1077s` -> `0.1084s` (`+0.0006s`, `+0.57%`)

Attribution on the same profiling mode after the accepted change:
- `classification`: `0.1562s` -> `0.1268s`
- `arrangement_pairs`: `0.0732s` -> `0.0751s`
- `pair_stats` stayed effectively the same, which matches the accepted change: the win came from eliminating duplicate polygon candidate traversal inside classification, not from the arrangement broad phase.
