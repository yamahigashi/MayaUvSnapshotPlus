2026-04-20 09:51: Rebuilt release binary on the current head.
2026-04-20 09:52: Single probe run measured `prepare_total=0.5713s`, `arrangement=0.2881s`.
2026-04-20 09:53: Fresh 3-run baseline measured `prepare_total=0.6154s / 0.5433s / 0.5446s` and `arrangement=0.3041s / 0.2839s / 0.2797s`; first run was noisy but the mean stayed near the prior post-change range.
2026-04-20 09:53: Detail profile measured `arrangement_pairs=0.0803s`, `arrangement_finalize=0.1232s`, `pair duplicates=2045859`, `split duplicates=44480`.
2026-04-20 09:54: Selected finalize-side lookup/rebuild overhead as the next optimization target.
2026-04-20 09:57: Iteration 1 replaced finalize-time `HashMap<CanonicalSegment, Vec<_>>` lookups with index-addressed split storage keyed by `original_segments`.
2026-04-20 09:58: Iteration 1 passed `cargo build --release` and `cargo test --manifest-path rust/edge_drawer/Cargo.toml test_arrangement_ -- --nocapture` (test build emitted the existing incremental-permission warning only).
2026-04-20 09:59: Iteration 1 measurement improved `prepare_total` mean to `0.5237s` and the attribution sample reduced `arrangement_finalize` to `0.1081s`.
2026-04-20 10:02: Iteration 2 moved group-segment index mapping into input construction so finalize no longer rebuilds the `segment -> index` map per arrangement pass.
2026-04-20 10:03: Iteration 2 passed the same release build and arrangement tests.
2026-04-20 10:04: Iteration 2 measurement improved `prepare_total` mean to `0.5060s`; attribution measured `arrangement_finalize=0.0780s` with unchanged pair/split counts.
2026-04-20 10:05: Final validation on the same code measured `prepare_total=0.5728s / 0.5491s / 0.5315s` (`0.5511s` mean), confirming an end-to-end win versus baseline despite a slower first run.
