# Result
- Workload: `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`
- Accepted changes:
  - Iteration 1: replace finalize-time split-part storage keyed by `CanonicalSegment` with index-addressed storage keyed by `original_segments`
  - Iteration 2: precompute `group_segment_indices` in arrangement input construction and reuse them during finalize instead of rebuilding the segment-index map per call

- Primary metric `prepare_total`:
  - Baseline mean: `0.5678s`
  - Best post-change sample: `0.5060s` (`-0.0618s`, `-10.88%`)
  - Final validation mean: `0.5511s` (`-0.0166s`, `-2.93%`)

- Supporting metrics on final validation versus baseline:
  - `arrangement`: `0.2892s` -> `0.2717s` (`-0.0175s`, `-6.06%`)
  - `classification`: `0.1960s` -> `0.2039s` (`+0.0080s`, `+4.07%`)
  - `path_build`: `0.0823s` -> `0.0749s` (`-0.0074s`, `-8.95%`)
  - `render_raster`: `0.1043s` -> `0.1148s` (`+0.0105s`, `+10.07%`)

- Attribution on the accepted final code:
  - `arrangement_pairs`: `0.0803s` -> `0.0798s`
  - `arrangement_finalize`: `0.1232s` -> `0.0780s` (`-0.0452s`, `-36.69%`)
  - Pair and split duplicate counts were unchanged, which supports the conclusion that the win came from cheaper finalize-side reconstruction rather than fewer candidate checks.

The benchmark stayed noisy on the first run of each sample. Because the final validation still improved the primary metric on the same workload, the change is accepted, but the larger warm-run gain should be treated as optimistic rather than guaranteed.
