<proposed_plan>
Summary
- Re-measure the current Maya UI workload with `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"` and continue from the latest accepted execplan.
- Remove topology snapshot collection when the current UI settings require neither edge drawing nor padding warnings, because the baseline shows `group_count=0`, `line_count=0`, and almost all time is still spent building polygon buffers.

Key Changes
- Add a small settings helper in `python/uv_snapshot_edge_drawer/ui.py` that determines whether any topology snapshot is required for the current request.
- Fast-path `_build_snapshot_json()` so it skips `drawer.get_mesh_topology_snapshot()` entirely when both edge drawing and padding warnings are disabled.
- Apply the same fast-path to preview request capture so the preview path does not warm or wait on topology that will not be consumed.
- Re-run the same Maya benchmark and compare `build_snapshot_payload` first, with `collect_snapshots_faces` as supporting evidence.

Public APIs / Internal Interfaces
- No public API changes.
- Internal UI payload-building flow gains a new helper: `_settings_need_topology_snapshot(settings)`.

Test Plan
- Run `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"` before and after the change.
- Confirm the post-change profile reports zero `collect_snapshots*` time for the benchmark's no-edge/no-warning configuration.
- If the first post-change run is noisy, repeat the same command for validation before accepting.

Assumptions
- When all edge draw toggles are off and `padding_warning_enabled` is false, the correct rendered result is blank, so building polygon buffers is unnecessary work.
- The benchmark workload still reflects the same no-edge/no-warning configuration observed in the new baseline.
</proposed_plan>
