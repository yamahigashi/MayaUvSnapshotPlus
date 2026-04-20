Remaining bottleneck:
- For the benchmark exercised here, there is no longer a meaningful `build_snapshot_payload` bottleneck after the empty-payload fast path.

Best next move:
- If the next target is a real drawing workload, benchmark with at least one edge mode enabled and/or padding warnings enabled so the remaining hot path reflects actual topology usage.
- For that non-empty workload, continue from the Rust/Maya polygon-buffer investigations in the earlier execplans because topology extraction will dominate again once rendering is requested.

Rejected ideas during this run:
- No further Rust-side tuning was attempted in this iteration because the new baseline showed the dominant cost came from unnecessary work, not expensive required work.
