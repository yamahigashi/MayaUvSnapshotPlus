# Changelog

All notable changes to Maya UV Snapshot Plus will be documented in this file.

## [v2.1.0] - 2026-05-16

### Added

- Added UV Island Fill rendering with configurable opacity and padding.
- Added preview zoom controls with Ctrl+mouse wheel.
- Added Island Fill padding support for raster output.

### Changed

- Improved UV island detection by building fill regions from polygon unions.
- Improved Island Fill padding rasterization so padded regions are assigned by nearest island contour without draw-order-dependent overlap artifacts.
- Optimized Island Fill padding rendering for large snapshots.
- Improved preview display scaling behavior and cached preview pixmaps during zoom.
- Disabled internal edge display automatically when Island Fill is enabled from an off state.
- Collapsed UV Area Settings by default.

### Fixed

- Fixed cases where closed UV regions were not detected as islands around junction-heavy layouts.
- Fixed DPI scaling mismatch between the preview UI region and the rendered preview image.
- Fixed stale or mismatched preview image sizing during rapid Ctrl+wheel zoom.
- Fixed faint pre-padding island outlines remaining visible in padded raster fill output.

## [v2.0.0] - 2026-04-20

### Added

- Added Rust/PyO3 native edge drawing backend for faster snapshot generation.
- Added versioned Maya packaging layout for Maya 2022 through Maya 2027.
- Added release packaging scripts and GitHub Actions workflows.
- Added clipboard output support.

### Changed

- Improved preview and edge rendering performance on complex meshes.
- Improved edge classification robustness.
- Improved UV snapshot UI layout and edge appearance controls.
- Updated README installation guidance for v2 package releases.

### Fixed

- Fixed packaging workflow issues for versioned release zip output.
- Fixed CI packaging command setup.
- Skipped empty topology snapshot builds.
