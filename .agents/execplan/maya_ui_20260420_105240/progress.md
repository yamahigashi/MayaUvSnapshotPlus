2026-04-20 10:52 JST
- Continued from `.agents/execplan/maya_ui_20260420_085203/`, but re-ran the current baseline first with `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"`.
- Baseline means from the current tree:
- `small build_snapshot_payload=0.0032s`
- `medium build_snapshot_payload=0.0160s`
- `heavy build_snapshot_payload=0.0392s`
- Baseline attribution showed `group_count=0`, `line_count=0`, `padding_warning_enabled=False`, and almost all work still inside `collect_snapshots_faces`.

2026-04-20 10:53 JST
- Re-read the benchmark harness in `tests/perf_maya.py` and confirmed it exercises a no-edge/no-warning configuration.
- Identified a direct fast path: when both edge drawing and padding warnings are disabled, `_build_snapshot_json()` and preview capture were still forcing full topology snapshot collection even though the final payload was empty.

2026-04-20 10:53 JST
- Updated `python/uv_snapshot_edge_drawer/ui.py`:
- added `_settings_need_topology_snapshot(settings)`
- skipped `drawer.get_mesh_topology_snapshot(...)` in `_build_snapshot_json()` when topology is not needed
- skipped cached topology lookup and warmup gating in `_capture_preview_request()` for the same condition

2026-04-20 10:54 JST
- Re-ran `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"` after the change.
- First post-change means:
- `small=0.00007s`
- `medium=0.00008s`
- `heavy=0.00007s`
- `collect_snapshots` dropped to ~`5-10us` and `polygon_count` dropped from tens or hundreds of thousands to `0`.

2026-04-20 10:54 JST
- Ran the same benchmark again as validation.
- Validation means:
- `small=0.00009s`
- `medium=0.00007s`
- `heavy=0.00009s`
- The second run confirmed the fast path is stable and the previous `collect_snapshots_faces` bottleneck is gone for this workload.
