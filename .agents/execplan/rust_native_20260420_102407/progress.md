2026-04-20 10:24:07 +0900
- Created run directory for a new Rust-native performance iteration.

2026-04-20 10:25 +0900
- Built `rust/edge_drawer` in release mode with `cargo build --release --manifest-path rust/edge_drawer/Cargo.toml`.

2026-04-20 10:26 +0900
- Baseline workload: `EDGE_DRAWER_PROFILE=1 rust/edge_drawer/target/release/edge_drawer <out.png> 2048 2048 .agents/execplan/rust_native_20260420_091318/large_sanitized_regen.json`
- Baseline runs:
  - `prepare_total`: `0.4867s`, `0.5040s`, `0.4993s`
  - `arrangement`: `0.2065s`, `0.2066s`, `0.2017s`
  - `classification`: `0.1800s`, `0.1884s`, `0.1896s`
  - `path_build`: `0.0958s`, `0.1052s`, `0.1048s`
  - `render_raster`: `0.1277s`, `0.1159s`, `0.1288s`

2026-04-20 10:27 +0900
- Detailed attribution with `EDGE_DRAWER_PROFILE=1 EDGE_DRAWER_ARRANGEMENT_PROFILE=1 EDGE_DRAWER_PAIR_PROFILE=1`:
  - `arrangement_collect_inputs`: `0.0402s`
  - `arrangement_setup`: `0.0106s`
  - `arrangement_pairs`: `0.0804s`
  - `arrangement_finalize`: `0.0183s`
  - `pair_stats`: `cells=856350 dense_candidates=4519211 duplicates=2045859 overlaps=1162169`
- Observation: the current heuristic caps the candidate-pair grid at `512`, which is likely too coarse for this workload size (`102,833` segments).

2026-04-20 10:31 +0900
- Experimented with raising the candidate-pair grid cap from `512` to `1024`.
- Result: rejected. Detail profile regressed badly:
  - `pair_stats`: `cells=2158506 dense_candidates=5726210 duplicates=4013801 overlaps=1162169`
  - `arrangement_pairs`: `0.1231s`
  - `prepare_total`: `0.5390s`
- Conclusion: finer-than-current grid increases multi-cell duplicate work faster than it reduces per-cell density on this workload.

2026-04-20 10:38 +0900
- Restored the heuristic and added an early reject in `register_pair_splits()` when `o1/o2` prove the right segment lies strictly on one side of the left segment.
- First validation on top of the restored `512` cap did not improve the baseline enough to accept the change alone.

2026-04-20 10:41 +0900
- Explored the opposite grid direction by lowering the cap from `512` to `256` while keeping the early reject.
- Detail probe improved:
  - `pair_stats`: `cells=436402 dense_candidates=5606060 duplicates=1289889 overlaps=1162169`
  - `arrangement_pairs`: `0.0760s`
  - `prepare_total`: `0.3791s`

2026-04-20 10:43 +0900
- Final 3-run validation accepted the `256` cap plus the early reject.
- Final runs:
  - `prepare_total`: `0.4871s`, `0.4815s`, `0.4986s`
  - `arrangement`: `0.1906s`, `0.1897s`, `0.1886s`
  - `classification`: `0.1743s`, `0.1791s`, `0.1844s`
  - `path_build`: `0.1182s`, `0.1089s`, `0.1220s`
  - `render_raster`: `0.1108s`, `0.1110s`, `0.1099s`
- Accepted comparison vs baseline means:
  - `prepare_total`: `0.4967s` -> `0.4891s` (`-0.0076s`, `-1.53%`)
  - `arrangement`: `0.2049s` -> `0.1896s` (`-0.0153s`, `-7.47%`)
