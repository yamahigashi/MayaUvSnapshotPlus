Baseline command: `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"`

Validation command: `cmd.exe /c "set MAYA_UV_SNAPSHOT_PROFILE=1 && test.bat"`

Primary metric comparison (`build_snapshot_payload` mean):
- `small`: `0.00315s` -> `0.00009s` (`-0.00306s`, `-97.3%`)
- `medium`: `0.01604s` -> `0.00007s` (`-0.01597s`, `-99.6%`)
- `heavy`: `0.03923s` -> `0.00009s` (`-0.03914s`, `-99.8%`)

Supporting metric comparison:
- `heavy collect_snapshots_faces`: `0.03416s` -> `0.00000s` for the validated no-topology path
- `heavy polygon_count`: `149153` -> `0`
- `heavy polygon_point_count`: `526219` -> `0`

What changed:
- Added a UI-level fast path that skips topology snapshot collection when the current settings require neither edge drawing nor padding warnings.
- Applied the same logic to preview request capture so the preview path does not warm the cache for an empty render request.

Interpretation:
- The benchmark harness is currently a no-edge/no-warning workload, so the previous payload time was almost entirely wasted polygon collection.
- Removing the work, rather than micro-optimizing it, produced the largest improvement seen in the Maya UI path so far.
