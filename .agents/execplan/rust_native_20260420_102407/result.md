# Result
- Workload: `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`
- Accepted changes:
  - lower the candidate-pair grid resolution cap from `512` to `256`
  - add an early reject in `register_pair_splits()` when the first two orientations already prove non-intersection
- Rejected change:
  - raising the candidate-pair grid cap to `1024` increased duplicate work and regressed the workload

- Primary metric `prepare_total`: `0.4967s` -> `0.4891s` (`-0.0076s`, `-1.53%`)
- `arrangement`: `0.2049s` -> `0.1896s` (`-0.0153s`, `-7.47%`)
- `classification`: `0.1860s` -> `0.1793s` (`-0.0067s`, `-3.62%`)
- `path_build`: `0.1019s` -> `0.1164s` (`+0.0144s`, `+14.16%`)
- `render_raster`: `0.1241s` -> `0.1106s` (`-0.0136s`, `-10.93%`)

Supporting attribution for the accepted configuration:
- `pair_stats`: `cells=436402`, `dense_candidates=5606060`, `duplicates=1289889`, `overlaps=1162169`
- `arrangement_pairs`: `0.0804s` baseline detail sample -> `0.0861s` accepted detail sample, while the accepted 3-run end-to-end sample still improved overall

Interpretation:
- The coarse-to-medium grid shift reduced per-segment cell visitation and duplicate revisits enough to improve the real workload, even though raw dense candidates increased.
- This workload remains noisy. Acceptance is based on the repeated non-detail 3-run comparison, not on a single attribution sample.
