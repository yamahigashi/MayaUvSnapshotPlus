use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::Deserialize;
use svg::node::element::path::Data;
use svg::node::element::Path as SvgPath;
use svg::Document;
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

type BoxError = Box<dyn Error + Send + Sync>;

const QUANTIZE_SCALE: f32 = 1_000.0;
const SAMPLE_EPSILON: f32 = 1e-4;
const AREA_EPSILON: f32 = 1e-8;
const DEFAULT_WARNING_COLOR: [u8; 4] = [255, 64, 64, 255];
const DEFAULT_WARNING_WIDTH: f32 = 4.0;

fn default_true() -> bool {
    true
}

#[derive(Debug)]
pub struct Config {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub edges: Vec<Edges>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PaddingWarningConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_padding_pixels")]
    padding_pixels: f32,
    #[serde(default = "default_warning_width")]
    warning_width: f32,
    #[serde(default = "default_warning_color")]
    warning_color: [u8; 4],
}

#[derive(Debug, Clone)]
pub struct DrawerPayload {
    pub edges: Vec<Edges>,
    pub polygons: Vec<Polygon>,
    pub padding_warning: Option<PaddingWarningConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Edges {
    #[serde(default)]
    line_width: Option<f32>,
    #[serde(default)]
    internal_width: Option<f32>,
    #[serde(default)]
    outline_width: Option<f32>,
    #[serde(default)]
    line_color: Option<[u8; 4]>,
    #[serde(default)]
    internal_color: Option<[u8; 4]>,
    #[serde(default)]
    outline_color: Option<[u8; 4]>,
    #[serde(default = "default_true")]
    draw_outline: bool,
    #[serde(default = "default_true")]
    draw_internal: bool,
    #[serde(default)]
    hide_internal: Option<bool>,
    lines: Vec<Edge>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Edge {
    uv1: [f32; 2],
    uv2: [f32; 2],
}

#[derive(Debug, Clone, Deserialize)]
pub struct Polygon {
    points: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct QPoint {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy, Debug)]
struct SegmentInfo {
    a: QPoint,
    b: QPoint,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CanonicalSegment {
    start: QPoint,
    end: QPoint,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct DirectedEdge {
    from: QPoint,
    to: QPoint,
}

#[derive(Clone, Debug)]
struct PreparedGroup {
    line_width: f32,
    line_color: [u8; 4],
    paths: Vec<Vec<QPoint>>,
}

#[derive(Clone, Debug)]
struct PreparedDrawing {
    groups: Vec<PreparedGroup>,
}

#[derive(Clone, Debug)]
struct DrawStyle {
    internal_width: f32,
    outline_width: f32,
    internal_color: [u8; 4],
    outline_color: [u8; 4],
    draw_outline: bool,
    draw_internal: bool,
}

#[derive(Clone, Debug)]
struct FaceLoop {
    points: Vec<QPoint>,
    area: f32,
    component_id: usize,
}

#[derive(Clone, Debug)]
struct SegmentArrangement {
    segments: Vec<CanonicalSegment>,
    point_positions: HashMap<QPoint, [f32; 2]>,
    group_segments: Vec<Vec<CanonicalSegment>>,
}

#[derive(Clone, Debug)]
struct CompactPayload {
    styles: Vec<DrawStyle>,
    arrangement_input_segments: Vec<CanonicalSegment>,
    arrangement_input_group_segments: Vec<Vec<CanonicalSegment>>,
    point_positions: HashMap<QPoint, [f32; 2]>,
    polygons: Vec<Polygon>,
    padding_warning: Option<PaddingWarningConfig>,
}

#[derive(Clone, Copy, Debug)]
struct SegmentBounds {
    min: [f32; 2],
    max: [f32; 2],
}

#[derive(Clone, Debug)]
struct UniformGridIndex {
    min: [f32; 2],
    cell_size: [f32; 2],
    resolution: u32,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

#[derive(Clone, Debug)]
struct IndexedPolygon {
    points: Vec<[f32; 2]>,
    bounds: SegmentBounds,
}

#[derive(Clone, Debug)]
struct PolygonIndex {
    polygons: Vec<IndexedPolygon>,
    grid: UniformGridIndex,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SamplePointKey {
    x: i64,
    y: i64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum BucketKind {
    Internal,
    Outline,
    Overlay,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct StyleBucketKey {
    kind: BucketKind,
    line_width_bits: u32,
    line_color: [u8; 4],
}

#[derive(Clone, Debug)]
struct StyleBucket {
    line_width: f32,
    line_color: [u8; 4],
    segments: Vec<SegmentInfo>,
}

fn profile_enabled() -> bool {
    std::env::var("EDGE_DRAWER_PROFILE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn log_profile(label: &str, started_at: Instant) {
    if profile_enabled() {
        eprintln!("edge_drawer: {} {:.4}s", label, started_at.elapsed().as_secs_f64());
    }
}

fn default_padding_pixels() -> f32 {
    8.0
}

fn default_warning_color() -> [u8; 4] {
    DEFAULT_WARNING_COLOR
}

fn default_warning_width() -> f32 {
    DEFAULT_WARNING_WIDTH
}

pub fn parse_edges_json(edges_json: &str) -> Result<Vec<Edges>, BoxError> {
    Ok(parse_drawer_payload(edges_json)?.edges)
}

pub fn parse_drawer_payload(edges_json: &str) -> Result<DrawerPayload, BoxError> {
    let payload_value: serde_json::Value = serde_json::from_str(edges_json)?;

    if payload_value.is_array() {
        let edges = serde_json::from_value(payload_value)?;
        return Ok(DrawerPayload {
            edges,
            polygons: Vec::new(),
            padding_warning: None,
        });
    }

    let payload_object = payload_value
        .as_object()
        .ok_or_else(|| "Expected payload to be an array or object".to_string())?;

    let edges_value = payload_object
        .get("edges")
        .cloned()
        .ok_or_else(|| "Expected object payload to contain 'edges'".to_string())?;
    let edges = serde_json::from_value(edges_value)?;
    let polygons = match payload_object.get("polygons").cloned() {
        Some(value) => serde_json::from_value(value)?,
        None => Vec::new(),
    };
    let padding_warning = match payload_object.get("padding_warning").cloned() {
        Some(value) => Some(serde_json::from_value(value)?),
        None => None,
    };

    Ok(DrawerPayload {
        edges,
        polygons,
        padding_warning,
    })
}

pub fn load_edges_input(edges_arg: &str) -> Result<DrawerPayload, BoxError> {
    if Path::new(edges_arg).exists() {
        let file_contents = fs::read_to_string(edges_arg)?;
        parse_drawer_payload(&file_contents)
    } else {
        parse_drawer_payload(edges_arg)
    }
}

pub fn draw_to_path(
    image_path: &Path,
    width: u32,
    height: u32,
    edges_json: &str,
) -> Result<(), BoxError> {
    let payload = parse_drawer_payload(edges_json)?;
    draw_to_path_from_payload(image_path, width, height, &payload)
}

pub fn draw_to_path_from_input(
    image_path: &Path,
    width: u32,
    height: u32,
    edges_input: &str,
) -> Result<(), BoxError> {
    let payload = load_edges_input(edges_input)?;
    draw_to_path_from_payload(image_path, width, height, &payload)
}

pub fn draw_to_path_from_payload(
    image_path: &Path,
    width: u32,
    height: u32,
    payload: &DrawerPayload,
) -> Result<(), BoxError> {
    let prepare_started_at = Instant::now();
    let prepared = prepare_drawing(
        &payload.edges,
        &payload.polygons,
        width,
        height,
        payload.padding_warning.as_ref(),
    );
    log_profile("prepare_total", prepare_started_at);

    if image_path.extension().and_then(|s| s.to_str()) == Some("svg") {
        let render_started_at = Instant::now();
        let document = draw_edges_svg(&prepared, width, height);
        log_profile("render_svg", render_started_at);
        save_svg(&document, image_path)?;
    } else {
        let render_started_at = Instant::now();
        let pixmap = draw_edges_raster(&prepared, width, height)?;
        log_profile("render_raster", render_started_at);
        save_image(&pixmap, image_path)?;
    }

    Ok(())
}

pub fn draw_to_path_from_edges(
    image_path: &Path,
    width: u32,
    height: u32,
    edges: &[Edges],
) -> Result<(), BoxError> {
    draw_to_path_from_payload(
        image_path,
        width,
        height,
        &DrawerPayload {
            edges: edges.to_vec(),
            polygons: Vec::new(),
            padding_warning: None,
        },
    )
}

fn prepare_drawing(
    edges: &[Edges],
    polygons: &[Polygon],
    width: u32,
    height: u32,
    padding_warning: Option<&PaddingWarningConfig>,
) -> PreparedDrawing {
    let arrangement_started_at = Instant::now();
    let styles = edges
        .iter()
        .map(|group| DrawStyle {
            internal_width: group.effective_internal_width().max(0.0),
            outline_width: group.effective_outline_width().max(0.0),
            internal_color: group.effective_internal_color(),
            outline_color: group.effective_outline_color(),
            draw_outline: group.effective_draw_outline(),
            draw_internal: group.effective_draw_internal(),
        })
        .collect::<Vec<_>>();
    let arrangement = build_segment_arrangement(edges);
    log_profile("arrangement", arrangement_started_at);

    let classification_started_at = Instant::now();
    let (internal_segments, outline_segments) =
        classify_segments(&arrangement.segments, &arrangement.point_positions, polygons);
    log_profile("classification", classification_started_at);

    let warning_started_at = Instant::now();
    let warning_segments = detect_padding_warning_segments(
        &outline_segments,
        &arrangement.point_positions,
        width,
        height,
        padding_warning,
    );
    log_profile("warning", warning_started_at);

    let path_started_at = Instant::now();
    let mut normal_bucket_order = Vec::new();
    let mut overlay_bucket_order = Vec::new();
    let mut normal_buckets = HashMap::new();
    let mut overlay_buckets = HashMap::new();

    for (style, group_segments) in styles.iter().zip(arrangement.group_segments.iter()) {
        append_bucket_segments(
            style,
            group_segments,
            &internal_segments,
            &outline_segments,
            &warning_segments,
            padding_warning,
            &mut normal_buckets,
            &mut normal_bucket_order,
            &mut overlay_buckets,
            &mut overlay_bucket_order,
        );
    }

    let mut groups = materialize_style_buckets(&normal_bucket_order, &mut normal_buckets);
    groups.extend(materialize_style_buckets(
        &overlay_bucket_order,
        &mut overlay_buckets,
    ));
    log_profile("path_build", path_started_at);

    PreparedDrawing { groups }
}

fn prepare_drawing_from_compact(
    payload: &CompactPayload,
    width: u32,
    height: u32,
) -> PreparedDrawing {
    let arrangement_started_at = Instant::now();
    let arrangement = build_segment_arrangement_from_parts(
        payload.arrangement_input_segments.clone(),
        payload.point_positions.clone(),
        &payload.arrangement_input_group_segments,
    );
    log_profile("arrangement", arrangement_started_at);

    let classification_started_at = Instant::now();
    let (internal_segments, outline_segments) =
        classify_segments(&arrangement.segments, &arrangement.point_positions, &payload.polygons);
    log_profile("classification", classification_started_at);

    let warning_started_at = Instant::now();
    let warning_segments = detect_padding_warning_segments(
        &outline_segments,
        &arrangement.point_positions,
        width,
        height,
        payload.padding_warning.as_ref(),
    );
    log_profile("warning", warning_started_at);

    let path_started_at = Instant::now();
    let mut normal_bucket_order = Vec::new();
    let mut overlay_bucket_order = Vec::new();
    let mut normal_buckets = HashMap::new();
    let mut overlay_buckets = HashMap::new();

    for (style, group_segments) in payload.styles.iter().zip(arrangement.group_segments.iter()) {
        append_bucket_segments(
            style,
            group_segments,
            &internal_segments,
            &outline_segments,
            &warning_segments,
            payload.padding_warning.as_ref(),
            &mut normal_buckets,
            &mut normal_bucket_order,
            &mut overlay_buckets,
            &mut overlay_bucket_order,
        );
    }

    let mut groups = materialize_style_buckets(&normal_bucket_order, &mut normal_buckets);
    groups.extend(materialize_style_buckets(
        &overlay_bucket_order,
        &mut overlay_buckets,
    ));
    log_profile("path_build", path_started_at);

    PreparedDrawing { groups }
}

fn collect_point_positions(edges: &[Edges]) -> HashMap<QPoint, [f32; 2]> {
    let mut positions = HashMap::new();
    for group in edges {
        for line in &group.lines {
            positions
                .entry(quantize_point(line.uv1))
                .or_insert(line.uv1);
            positions
                .entry(quantize_point(line.uv2))
                .or_insert(line.uv2);
        }
    }
    positions
}

fn build_segment_arrangement(edges: &[Edges]) -> SegmentArrangement {
    let point_positions = collect_point_positions(edges);
    let original_segments = collect_unique_segments(edges);
    let mut group_segments = Vec::with_capacity(edges.len());
    for group in edges {
        let mut group_set = HashSet::new();
        for line in &group.lines {
            let Some(original) = canonical_segment(quantize_point(line.uv1), quantize_point(line.uv2))
            else {
                continue;
            };
            group_set.insert(original);
        }
        let mut parts = group_set.into_iter().collect::<Vec<_>>();
        parts.sort_by_key(|segment| (segment.start, segment.end));
        group_segments.push(parts);
    }

    build_segment_arrangement_from_parts(original_segments, point_positions, &group_segments)
}

fn build_segment_arrangement_from_parts(
    original_segments: Vec<CanonicalSegment>,
    mut point_positions: HashMap<QPoint, [f32; 2]>,
    original_group_segments: &[Vec<CanonicalSegment>],
) -> SegmentArrangement {
    let mut split_points: HashMap<CanonicalSegment, Vec<QPoint>> = HashMap::new();
    let segment_bounds = original_segments
        .iter()
        .map(|segment| segment_bounds_uv(*segment, &point_positions))
        .collect::<Vec<_>>();

    for (left_index, right_index) in collect_candidate_pairs(&segment_bounds, 0.0) {
        let left = original_segments[left_index];
        let right = original_segments[right_index];
        let left_start = point_positions[&left.start];
        let left_end = point_positions[&left.end];
        let right_start = point_positions[&right.start];
        let right_end = point_positions[&right.end];

        for point in split_intersection_points(left_start, left_end, right_start, right_end) {
            let quantized = quantize_point(point);
            point_positions.entry(quantized).or_insert(point);
            append_split_point(&mut split_points, left, quantized);
            append_split_point(&mut split_points, right, quantized);
        }
    }

    let mut split_segments_by_original: HashMap<CanonicalSegment, Vec<CanonicalSegment>> = HashMap::new();
    let mut segments_set = HashSet::new();
    for segment in &original_segments {
        let parts = split_segment(*segment, split_points.get(segment), &point_positions);
        for part in &parts {
            segments_set.insert(*part);
        }
        split_segments_by_original.insert(*segment, parts);
    }

    let mut segments = segments_set.into_iter().collect::<Vec<_>>();
    segments.sort_by_key(|segment| (segment.start, segment.end));

    let mut group_segments = Vec::with_capacity(original_group_segments.len());
    for original_group in original_group_segments {
        let mut group_set = HashSet::new();
        for original in original_group {
            if let Some(parts) = split_segments_by_original.get(original) {
                group_set.extend(parts.iter().copied());
            }
        }
        let mut parts = group_set.into_iter().collect::<Vec<_>>();
        parts.sort_by_key(|segment| (segment.start, segment.end));
        group_segments.push(parts);
    }

    SegmentArrangement {
        segments,
        point_positions,
        group_segments,
    }
}

fn append_split_point(
    split_points: &mut HashMap<CanonicalSegment, Vec<QPoint>>,
    segment: CanonicalSegment,
    point: QPoint,
) {
    let points = split_points.entry(segment).or_default();
    if !points.contains(&point) {
        points.push(point);
    }
}

fn split_intersection_points(
    a0: [f32; 2],
    a1: [f32; 2],
    b0: [f32; 2],
    b1: [f32; 2],
) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    if colinear_segments(a0, a1, b0, b1) {
        for point in [a0, a1, b0, b1] {
            if point_on_segment(point, a0, a1) && point_on_segment(point, b0, b1) {
                points.push(point);
            }
        }
        dedupe_points(points)
    } else if let Some(point) = segment_intersection_point(a0, a1, b0, b1) {
        vec![point]
    } else {
        Vec::new()
    }
}

fn split_segment(
    original: CanonicalSegment,
    split_points: Option<&Vec<QPoint>>,
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> Vec<CanonicalSegment> {
    let start = point_positions[&original.start];
    let end = point_positions[&original.end];
    let direction = [end[0] - start[0], end[1] - start[1]];
    let len_sq = direction[0] * direction[0] + direction[1] * direction[1];
    if len_sq <= AREA_EPSILON {
        return Vec::new();
    }

    let mut ordered = vec![original.start, original.end];
    if let Some(split_points) = split_points {
        ordered.extend(split_points.iter().copied());
    }
    ordered.sort_by(|lhs, rhs| {
        segment_parameter(point_positions[lhs], start, end)
            .total_cmp(&segment_parameter(point_positions[rhs], start, end))
    });
    ordered.dedup();

    let mut segments = Vec::new();
    for window in ordered.windows(2) {
        if let Some(segment) = canonical_segment(window[0], window[1]) {
            segments.push(segment);
        }
    }
    segments
}

fn segment_parameter(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len_sq = dx * dx + dy * dy;
    if len_sq <= AREA_EPSILON {
        return 0.0;
    }
    ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / len_sq
}

fn dedupe_points(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for point in points {
        let quantized = quantize_point(point);
        if seen.insert(quantized) {
            result.push(point);
        }
    }
    result
}

#[cfg(test)]
fn classify_visible_segments(
    edges: &[Edges],
) -> HashSet<CanonicalSegment> {
    let arrangement = build_segment_arrangement(edges);
    if arrangement.segments.is_empty() {
        return HashSet::new();
    }

    let (internal_segments, outline_segments) =
        classify_segments(&arrangement.segments, &arrangement.point_positions, &[]);
    let mut visible = HashSet::new();

    for (group, group_segments) in edges.iter().zip(arrangement.group_segments.iter()) {
        for &segment in group_segments {
            if internal_segments.contains(&segment) {
                if group.effective_draw_internal() {
                    visible.insert(segment);
                }
            } else if outline_segments.contains(&segment) && group.effective_draw_outline() {
                visible.insert(segment);
            }
        }
    }

    visible
}

#[cfg(test)]
fn classify_warning_segments(
    edges: &[Edges],
    width: u32,
    height: u32,
    padding_pixels: f32,
) -> HashSet<CanonicalSegment> {
    let arrangement = build_segment_arrangement(edges);
    let (_internal_segments, outline_segments) =
        classify_segments(&arrangement.segments, &arrangement.point_positions, &[]);
    detect_padding_warning_segments(
        &outline_segments,
        &arrangement.point_positions,
        width,
        height,
        Some(&PaddingWarningConfig {
            enabled: true,
            padding_pixels,
            warning_width: DEFAULT_WARNING_WIDTH,
            warning_color: DEFAULT_WARNING_COLOR,
        }),
    )
}

fn append_bucket_segments(
    style: &DrawStyle,
    group_segments: &[CanonicalSegment],
    internal_segments: &HashSet<CanonicalSegment>,
    outline_segments_set: &HashSet<CanonicalSegment>,
    warning_segments: &HashSet<CanonicalSegment>,
    padding_warning: Option<&PaddingWarningConfig>,
    normal_buckets: &mut HashMap<StyleBucketKey, StyleBucket>,
    normal_bucket_order: &mut Vec<StyleBucketKey>,
    overlay_buckets: &mut HashMap<StyleBucketKey, StyleBucket>,
    overlay_bucket_order: &mut Vec<StyleBucketKey>,
) {
    let mut outline_segments = Vec::new();
    let mut internal_only_segments = Vec::new();

    for &canonical in group_segments {
        let segment = SegmentInfo {
            a: canonical.start,
            b: canonical.end,
        };
        if internal_segments.contains(&canonical) {
            if style.draw_internal {
                internal_only_segments.push(segment);
            }
        } else if outline_segments_set.contains(&canonical) && style.draw_outline {
            outline_segments.push(segment);
        }
    }

    if !internal_only_segments.is_empty() {
        append_style_bucket(
            normal_buckets,
            normal_bucket_order,
            BucketKind::Internal,
            style.internal_width,
            style.internal_color,
            false,
            &internal_only_segments,
        );
    }
    if !outline_segments.is_empty() {
        let outline_width = style.outline_width;
        append_style_bucket(
            normal_buckets,
            normal_bucket_order,
            BucketKind::Outline,
            outline_width,
            style.outline_color,
            false,
            &outline_segments,
        );

        if padding_warning
            .map(|warning| warning.enabled)
            .unwrap_or(false)
        {
            let warned_outline_segments = outline_segments
                .iter()
                .copied()
                .filter(|segment| {
                    canonical_segment(segment.a, segment.b)
                        .map(|canonical| warning_segments.contains(&canonical))
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>();

            if !warned_outline_segments.is_empty() {
                append_style_bucket(
                    overlay_buckets,
                    overlay_bucket_order,
                    BucketKind::Overlay,
                    padding_warning
                        .map(|warning| warning.warning_width.max(0.0))
                        .unwrap_or(outline_width),
                    padding_warning
                        .map(|warning| warning.warning_color)
                        .unwrap_or(DEFAULT_WARNING_COLOR),
                    true,
                    &warned_outline_segments,
                );
            }
        }
    }
}

fn append_style_bucket(
    buckets: &mut HashMap<StyleBucketKey, StyleBucket>,
    bucket_order: &mut Vec<StyleBucketKey>,
    kind: BucketKind,
    line_width: f32,
    line_color: [u8; 4],
    _is_overlay: bool,
    segments: &[SegmentInfo],
) {
    let key = StyleBucketKey {
        kind,
        line_width_bits: line_width.to_bits(),
        line_color,
    };
    if !buckets.contains_key(&key) {
        bucket_order.push(key);
        buckets.insert(
            key,
            StyleBucket {
                line_width,
                line_color,
                segments: Vec::new(),
            },
        );
    }
    if let Some(bucket) = buckets.get_mut(&key) {
        bucket.segments.extend_from_slice(segments);
    }
}

fn materialize_style_buckets(
    bucket_order: &[StyleBucketKey],
    buckets: &mut HashMap<StyleBucketKey, StyleBucket>,
) -> Vec<PreparedGroup> {
    let mut groups = Vec::new();
    for key in bucket_order {
        let Some(bucket) = buckets.remove(key) else {
            continue;
        };
        groups.push(PreparedGroup {
            line_width: bucket.line_width,
            line_color: bucket.line_color,
            paths: build_paths_from_segments(&bucket.segments),
        });
    }
    groups
}

fn collect_unique_segments(edges: &[Edges]) -> Vec<CanonicalSegment> {
    let mut set = HashSet::new();
    for group in edges {
        for line in &group.lines {
            if let Some(segment) =
                canonical_segment(quantize_point(line.uv1), quantize_point(line.uv2))
            {
                set.insert(segment);
            }
        }
    }
    let mut segments = set.into_iter().collect::<Vec<_>>();
    segments.sort_by_key(|segment| (segment.start, segment.end));
    segments
}

fn default_grid_resolution(count: usize) -> u32 {
    if count <= 1 {
        return 16;
    }

    let target = (count as f32).sqrt().ceil() as u32;
    target.next_power_of_two().clamp(16, 256)
}

fn combine_bounds(bounds: &[SegmentBounds], expand: f32) -> ([f32; 2], [f32; 2]) {
    let mut min = [f32::INFINITY, f32::INFINITY];
    let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY];

    for bounds in bounds {
        min[0] = min[0].min(bounds.min[0] - expand);
        min[1] = min[1].min(bounds.min[1] - expand);
        max[0] = max[0].max(bounds.max[0] + expand);
        max[1] = max[1].max(bounds.max[1] + expand);
    }

    if !min[0].is_finite() {
        return ([0.0, 0.0], [1.0, 1.0]);
    }

    if (max[0] - min[0]).abs() <= f32::EPSILON {
        max[0] = min[0] + 1.0;
    }
    if (max[1] - min[1]).abs() <= f32::EPSILON {
        max[1] = min[1] + 1.0;
    }

    (min, max)
}

fn build_uniform_grid(bounds: &[SegmentBounds], expand: f32) -> UniformGridIndex {
    let resolution = default_grid_resolution(bounds.len());
    let (min, max) = combine_bounds(bounds, expand);
    let cell_size = [
        (max[0] - min[0]) / resolution as f32,
        (max[1] - min[1]) / resolution as f32,
    ];
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for (idx, bounds) in bounds.iter().enumerate() {
        let min_x = cell_coord(bounds.min[0] - expand, min[0], cell_size[0], resolution);
        let max_x = cell_coord(bounds.max[0] + expand, min[0], cell_size[0], resolution);
        let min_y = cell_coord(bounds.min[1] - expand, min[1], cell_size[1], resolution);
        let max_y = cell_coord(bounds.max[1] + expand, min[1], cell_size[1], resolution);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                cells.entry((x, y)).or_default().push(idx);
            }
        }
    }

    UniformGridIndex {
        min,
        cell_size,
        resolution,
        cells,
    }
}

fn collect_candidate_pairs(bounds: &[SegmentBounds], expand: f32) -> Vec<(usize, usize)> {
    if bounds.len() < 2 {
        return Vec::new();
    }

    let mut sorted_indices = (0..bounds.len()).collect::<Vec<_>>();
    sorted_indices.sort_by(|lhs, rhs| {
        bounds[*lhs].min[0]
            .total_cmp(&bounds[*rhs].min[0])
            .then_with(|| bounds[*lhs].max[0].total_cmp(&bounds[*rhs].max[0]))
    });

    let mut active = Vec::<usize>::new();
    let mut pairs = Vec::new();
    for &current in &sorted_indices {
        let current_min_x = bounds[current].min[0] - expand;
        active.retain(|idx| bounds[*idx].max[0] + expand >= current_min_x);

        for &candidate in &active {
            if bounds_overlap(bounds[current], bounds[candidate], expand) {
                let pair = if candidate < current {
                    (candidate, current)
                } else {
                    (current, candidate)
                };
                pairs.push(pair);
            }
        }

        active.push(current);
    }

    pairs
}

fn bounds_overlap(left: SegmentBounds, right: SegmentBounds, expand: f32) -> bool {
    left.min[0] - expand <= right.max[0] + expand
        && left.max[0] + expand >= right.min[0] - expand
        && left.min[1] - expand <= right.max[1] + expand
        && left.max[1] + expand >= right.min[1] - expand
}

fn cell_coord(value: f32, min: f32, cell_size: f32, resolution: u32) -> i32 {
    let normalized = if cell_size <= f32::EPSILON {
        0.0
    } else {
        (value - min) / cell_size
    };
    let scaled = normalized.floor() as i32;
    scaled.clamp(0, resolution as i32 - 1)
}

fn segment_bounds_uv(
    segment: CanonicalSegment,
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> SegmentBounds {
    let start = point_positions[&segment.start];
    let end = point_positions[&segment.end];
    SegmentBounds {
        min: [start[0].min(end[0]), start[1].min(end[1])],
        max: [start[0].max(end[0]), start[1].max(end[1])],
    }
}

fn segment_bounds_canvas(segment: CanonicalSegment, width: u32, height: u32) -> SegmentBounds {
    let start = to_canvas_point(segment.start, width, height);
    let end = to_canvas_point(segment.end, width, height);
    SegmentBounds {
        min: [start[0].min(end[0]), start[1].min(end[1])],
        max: [start[0].max(end[0]), start[1].max(end[1])],
    }
}

fn build_canvas_grid(
    segments: &[CanonicalSegment],
    width: u32,
    height: u32,
    expand_pixels: f32,
) -> UniformGridIndex {
    let resolution = default_grid_resolution(segments.len());
    let width_f = width.max(1) as f32;
    let height_f = height.max(1) as f32;
    let min = [0.0, 0.0];
    let cell_size = [
        width_f / resolution as f32,
        height_f / resolution as f32,
    ];
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for (idx, segment) in segments.iter().enumerate() {
        let bounds = segment_bounds_canvas(*segment, width, height);
        let min_x = cell_coord((bounds.min[0] - expand_pixels).max(0.0), 0.0, cell_size[0], resolution);
        let max_x = cell_coord((bounds.max[0] + expand_pixels).min(width_f), 0.0, cell_size[0], resolution);
        let min_y = cell_coord((bounds.min[1] - expand_pixels).max(0.0), 0.0, cell_size[1], resolution);
        let max_y = cell_coord((bounds.max[1] + expand_pixels).min(height_f), 0.0, cell_size[1], resolution);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                cells.entry((x, y)).or_default().push(idx);
            }
        }
    }

    UniformGridIndex {
        min,
        cell_size,
        resolution,
        cells,
    }
}

fn detect_padding_warning_segments(
    outline_segments: &HashSet<CanonicalSegment>,
    point_positions: &HashMap<QPoint, [f32; 2]>,
    width: u32,
    height: u32,
    padding_warning: Option<&PaddingWarningConfig>,
) -> HashSet<CanonicalSegment> {
    let Some(padding_warning) = padding_warning else {
        return HashSet::new();
    };
    if !padding_warning.enabled {
        return HashSet::new();
    }

    if outline_segments.is_empty() {
        return HashSet::new();
    }

    let outline_segments = {
        let mut segments = outline_segments.iter().copied().collect::<Vec<_>>();
        segments.sort_by_key(|segment| (segment.start, segment.end));
        segments
    };

    let outline_adjacency = build_adjacency(&outline_segments, point_positions);
    let outline_components = compute_components(&outline_adjacency);
    let mut warning_segments = HashSet::new();

    for segment in &outline_segments {
        if segment_border_distance_pixels(*segment, width, height) < padding_warning.padding_pixels
        {
            warning_segments.insert(*segment);
        }
    }

    let segment_component_ids = outline_segments
        .iter()
        .map(|segment| outline_components.get(&segment.start).copied().unwrap_or_default())
        .collect::<Vec<_>>();
    let canvas_grid = build_canvas_grid(
        &outline_segments,
        width,
        height,
        padding_warning.padding_pixels,
    );
    let mut candidate_pairs = HashSet::new();

    for indices in canvas_grid.cells.values() {
        for left_index in 0..indices.len() {
            for right_index in (left_index + 1)..indices.len() {
                let pair = if indices[left_index] < indices[right_index] {
                    (indices[left_index], indices[right_index])
                } else {
                    (indices[right_index], indices[left_index])
                };
                candidate_pairs.insert(pair);
            }
        }
    }

    let mut candidate_pairs = candidate_pairs.into_iter().collect::<Vec<_>>();
    candidate_pairs.sort_unstable();

    for (left_index, right_index) in candidate_pairs {
        if segment_component_ids[left_index] == segment_component_ids[right_index] {
            continue;
        }
        let left = outline_segments[left_index];
        let right = outline_segments[right_index];
        if segment_distance_pixels(left, right, width, height) < padding_warning.padding_pixels {
            warning_segments.insert(left);
            warning_segments.insert(right);
        }
    }

    warning_segments
}

fn segment_border_distance_pixels(
    segment: CanonicalSegment,
    width: u32,
    height: u32,
) -> f32 {
    let a = to_canvas_point(segment.start, width, height);
    let b = to_canvas_point(segment.end, width, height);
    let min_x = a[0].min(b[0]);
    let max_x = a[0].max(b[0]);
    let min_y = a[1].min(b[1]);
    let max_y = a[1].max(b[1]);
    let width = width as f32;
    let height = height as f32;

    min_x.min((width - max_x).max(0.0))
        .min(min_y)
        .min((height - max_y).max(0.0))
}

fn segment_distance_pixels(
    left: CanonicalSegment,
    right: CanonicalSegment,
    width: u32,
    height: u32,
) -> f32 {
    let a0 = to_canvas_point(left.start, width, height);
    let a1 = to_canvas_point(left.end, width, height);
    let b0 = to_canvas_point(right.start, width, height);
    let b1 = to_canvas_point(right.end, width, height);
    segment_distance_2d(a0, a1, b0, b1)
}

fn segment_distance_2d(a0: [f32; 2], a1: [f32; 2], b0: [f32; 2], b1: [f32; 2]) -> f32 {
    if segments_intersect(a0, a1, b0, b1) {
        return 0.0;
    }

    point_segment_distance(a0, b0, b1)
        .min(point_segment_distance(a1, b0, b1))
        .min(point_segment_distance(b0, a0, a1))
        .min(point_segment_distance(b1, a0, a1))
}

fn point_segment_distance(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let len_sq = dx * dx + dy * dy;
    if len_sq <= f32::EPSILON {
        return ((point[0] - start[0]).powi(2) + (point[1] - start[1]).powi(2)).sqrt();
    }

    let t = (((point[0] - start[0]) * dx) + ((point[1] - start[1]) * dy)) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let projected = [start[0] + t * dx, start[1] + t * dy];
    ((point[0] - projected[0]).powi(2) + (point[1] - projected[1]).powi(2)).sqrt()
}

fn segments_intersect(a0: [f32; 2], a1: [f32; 2], b0: [f32; 2], b1: [f32; 2]) -> bool {
    let o1 = orientation(a0, a1, b0);
    let o2 = orientation(a0, a1, b1);
    let o3 = orientation(b0, b1, a0);
    let o4 = orientation(b0, b1, a1);

    if o1.abs() <= AREA_EPSILON && on_segment(a0, b0, a1) {
        return true;
    }
    if o2.abs() <= AREA_EPSILON && on_segment(a0, b1, a1) {
        return true;
    }
    if o3.abs() <= AREA_EPSILON && on_segment(b0, a0, b1) {
        return true;
    }
    if o4.abs() <= AREA_EPSILON && on_segment(b0, a1, b1) {
        return true;
    }

    (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0)
}

fn orientation(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn on_segment(start: [f32; 2], point: [f32; 2], end: [f32; 2]) -> bool {
    point[0] >= start[0].min(end[0]) - AREA_EPSILON
        && point[0] <= start[0].max(end[0]) + AREA_EPSILON
        && point[1] >= start[1].min(end[1]) - AREA_EPSILON
        && point[1] <= start[1].max(end[1]) + AREA_EPSILON
}

fn colinear_segments(a0: [f32; 2], a1: [f32; 2], b0: [f32; 2], b1: [f32; 2]) -> bool {
    orientation(a0, a1, b0).abs() <= AREA_EPSILON && orientation(a0, a1, b1).abs() <= AREA_EPSILON
}

fn point_on_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> bool {
    orientation(start, end, point).abs() <= AREA_EPSILON && on_segment(start, point, end)
}

fn segment_intersection_point(
    a0: [f32; 2],
    a1: [f32; 2],
    b0: [f32; 2],
    b1: [f32; 2],
) -> Option<[f32; 2]> {
    if !segments_intersect(a0, a1, b0, b1) {
        return None;
    }

    let denominator =
        (a0[0] - a1[0]) * (b0[1] - b1[1]) - (a0[1] - a1[1]) * (b0[0] - b1[0]);
    if denominator.abs() <= AREA_EPSILON {
        for point in [a0, a1, b0, b1] {
            if point_on_segment(point, a0, a1) && point_on_segment(point, b0, b1) {
                return Some(point);
            }
        }
        return None;
    }

    let cross_a = a0[0] * a1[1] - a0[1] * a1[0];
    let cross_b = b0[0] * b1[1] - b0[1] * b1[0];
    Some([
        (cross_a * (b0[0] - b1[0]) - (a0[0] - a1[0]) * cross_b) / denominator,
        (cross_a * (b0[1] - b1[1]) - (a0[1] - a1[1]) * cross_b) / denominator,
    ])
}

fn classify_segments(
    unique_segments: &[CanonicalSegment],
    point_positions: &HashMap<QPoint, [f32; 2]>,
    polygons: &[Polygon],
) -> (HashSet<CanonicalSegment>, HashSet<CanonicalSegment>) {
    if polygons.is_empty() {
        return classify_segments_from_graph(unique_segments, point_positions);
    }

    let polygon_index = build_polygon_index(polygons);
    let mut sample_cache = HashMap::new();
    let mut internal_segments = HashSet::new();
    let mut outline_segments = HashSet::new();

    for &segment in unique_segments {
        let (left_inside, right_inside) =
            segment_side_states_with_polygons(
                segment,
                &polygon_index,
                point_positions,
                &mut sample_cache,
            );
        match (left_inside, right_inside) {
            (true, true) => {
                internal_segments.insert(segment);
            }
            (true, false) | (false, true) => {
                outline_segments.insert(segment);
            }
            (false, false) => {}
        }
    }

    (internal_segments, outline_segments)
}

fn classify_segments_from_graph(
    unique_segments: &[CanonicalSegment],
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> (HashSet<CanonicalSegment>, HashSet<CanonicalSegment>) {
    let adjacency = build_adjacency(unique_segments, point_positions);
    let component_map = compute_components(&adjacency);
    let faces = extract_filled_faces(unique_segments, &adjacency, &component_map, point_positions);
    let mut components_with_faces = HashSet::new();
    for face in &faces {
        components_with_faces.insert(face.component_id);
    }

    let mut internal_segments = HashSet::new();
    let mut outline_segments = HashSet::new();

    for &segment in unique_segments {
        let component_id = component_map.get(&segment.start).copied().unwrap_or_default();
        if !components_with_faces.contains(&component_id) {
            outline_segments.insert(segment);
            continue;
        }

        let (left_inside, right_inside) = segment_side_states(segment, &faces, point_positions);
        match (left_inside, right_inside) {
            (true, true) => {
                internal_segments.insert(segment);
            }
            (true, false) | (false, true) => {
                outline_segments.insert(segment);
            }
            (false, false) => {}
        }
    }

    (internal_segments, outline_segments)
}

fn segment_side_states_with_polygons(
    segment: CanonicalSegment,
    polygon_index: &PolygonIndex,
    point_positions: &HashMap<QPoint, [f32; 2]>,
    sample_cache: &mut HashMap<SamplePointKey, bool>,
) -> (bool, bool) {
    let a = point_positions[&segment.start];
    let b = point_positions[&segment.end];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
        return (false, false);
    }

    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let offset = [-(dy / len) * SAMPLE_EPSILON, (dx / len) * SAMPLE_EPSILON];
    let left = [mid[0] + offset[0], mid[1] + offset[1]];
    let right = [mid[0] - offset[0], mid[1] - offset[1]];

    (
        point_in_polygons(left, polygon_index, sample_cache),
        point_in_polygons(right, polygon_index, sample_cache),
    )
}

fn segment_side_states(
    segment: CanonicalSegment,
    faces: &[FaceLoop],
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> (bool, bool) {
    let a = point_positions[&segment.start];
    let b = point_positions[&segment.end];
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len <= f32::EPSILON {
        return (false, false);
    }

    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let offset = [-(dy / len) * SAMPLE_EPSILON, (dx / len) * SAMPLE_EPSILON];
    let left = [mid[0] + offset[0], mid[1] + offset[1]];
    let right = [mid[0] - offset[0], mid[1] - offset[1]];

    (
        point_in_faces(left, faces, point_positions),
        point_in_faces(right, faces, point_positions),
    )
}

fn build_adjacency(
    unique_segments: &[CanonicalSegment],
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> HashMap<QPoint, Vec<QPoint>> {
    let mut adjacency: HashMap<QPoint, Vec<QPoint>> = HashMap::new();
    for segment in unique_segments {
        adjacency
            .entry(segment.start)
            .or_default()
            .push(segment.end);
        adjacency
            .entry(segment.end)
            .or_default()
            .push(segment.start);
    }

    for (point, neighbors) in &mut adjacency {
        let origin = point_positions[point];
        neighbors.sort_by(|lhs, rhs| {
            let left = point_positions[lhs];
            let right = point_positions[rhs];
            let left_angle = (left[1] - origin[1]).atan2(left[0] - origin[0]);
            let right_angle = (right[1] - origin[1]).atan2(right[0] - origin[0]);
            left_angle.total_cmp(&right_angle)
        });
        neighbors.dedup();
    }

    adjacency
}

fn compute_components(adjacency: &HashMap<QPoint, Vec<QPoint>>) -> HashMap<QPoint, usize> {
    let mut components = HashMap::new();
    let mut next_id = 0usize;
    let mut starts = adjacency.keys().copied().collect::<Vec<_>>();
    starts.sort();

    for start in starts {
        if components.contains_key(&start) {
            continue;
        }

        let mut queue = VecDeque::from([start]);
        components.insert(start, next_id);

        while let Some(current) = queue.pop_front() {
            if let Some(neighbors) = adjacency.get(&current) {
                for &neighbor in neighbors {
                    if components.insert(neighbor, next_id).is_none() {
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        next_id += 1;
    }

    components
}

fn extract_filled_faces(
    unique_segments: &[CanonicalSegment],
    adjacency: &HashMap<QPoint, Vec<QPoint>>,
    component_map: &HashMap<QPoint, usize>,
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> Vec<FaceLoop> {
    let mut visited = HashSet::new();
    let mut faces = Vec::new();

    for segment in unique_segments {
        for directed in [
            DirectedEdge {
                from: segment.start,
                to: segment.end,
            },
            DirectedEdge {
                from: segment.end,
                to: segment.start,
            },
        ] {
            if visited.contains(&directed) {
                continue;
            }

            let mut current = directed;
            let mut loop_points = Vec::new();
            let mut seen_in_walk = HashSet::new();
            let mut closed = false;

            for _ in 0..(unique_segments.len() * 2 + 2) {
                if !seen_in_walk.insert(current) {
                    break;
                }
                visited.insert(current);
                loop_points.push(current.from);

                let Some(next) = next_half_edge(current, adjacency) else {
                    break;
                };
                if next == directed {
                    closed = true;
                    break;
                }
                current = next;
            }

            if !closed || loop_points.len() < 3 {
                continue;
            }

            let area = polygon_area(&loop_points, point_positions);
            if area.abs() <= AREA_EPSILON {
                continue;
            }

            let component_id = component_map
                .get(&loop_points[0])
                .copied()
                .unwrap_or_default();
            faces.push(FaceLoop {
                points: loop_points,
                area,
                component_id,
            });
        }
    }

    let mut outer_faces = HashMap::new();
    for (idx, face) in faces.iter().enumerate() {
        outer_faces
            .entry(face.component_id)
            .and_modify(|current: &mut usize| {
                if faces[*current].area.abs() < face.area.abs() {
                    *current = idx;
                }
            })
            .or_insert(idx);
    }

    faces
        .into_iter()
        .enumerate()
        .filter_map(|(idx, face)| {
            if outer_faces.get(&face.component_id).copied() == Some(idx) {
                None
            } else {
                Some(face)
            }
        })
        .collect()
}

fn next_half_edge(
    edge: DirectedEdge,
    adjacency: &HashMap<QPoint, Vec<QPoint>>,
) -> Option<DirectedEdge> {
    let neighbors = adjacency.get(&edge.to)?;
    if neighbors.len() < 2 {
        return None;
    }

    let incoming_index = neighbors.iter().position(|point| *point == edge.from)?;
    let next_index = if incoming_index == 0 {
        neighbors.len() - 1
    } else {
        incoming_index - 1
    };
    let next_neighbor = neighbors[next_index];
    Some(DirectedEdge {
        from: edge.to,
        to: next_neighbor,
    })
}

fn point_in_faces(
    point: [f32; 2],
    faces: &[FaceLoop],
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> bool {
    let mut inside = false;
    for face in faces {
        if point_in_polygon(point, &face.points, point_positions) {
            inside = !inside;
        }
    }
    inside
}

fn build_polygon_index(polygons: &[Polygon]) -> PolygonIndex {
    let indexed_polygons = polygons
        .iter()
        .map(|polygon| {
            let mut min = [f32::INFINITY, f32::INFINITY];
            let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY];
            for point in &polygon.points {
                min[0] = min[0].min(point[0]);
                min[1] = min[1].min(point[1]);
                max[0] = max[0].max(point[0]);
                max[1] = max[1].max(point[1]);
            }
            IndexedPolygon {
                points: polygon.points.clone(),
                bounds: SegmentBounds { min, max },
            }
        })
        .collect::<Vec<_>>();
    let bounds = indexed_polygons
        .iter()
        .map(|polygon| polygon.bounds)
        .collect::<Vec<_>>();
    let grid = build_uniform_grid(&bounds, 0.0);
    PolygonIndex {
        polygons: indexed_polygons,
        grid,
    }
}

fn sample_point_key(point: [f32; 2]) -> SamplePointKey {
    SamplePointKey {
        x: (point[0] * 1_000_000.0).round() as i64,
        y: (point[1] * 1_000_000.0).round() as i64,
    }
}

fn point_in_polygons(
    point: [f32; 2],
    polygon_index: &PolygonIndex,
    sample_cache: &mut HashMap<SamplePointKey, bool>,
) -> bool {
    let key = sample_point_key(point);
    if let Some(is_inside) = sample_cache.get(&key) {
        return *is_inside;
    }

    let mut is_inside = false;
    for &polygon_index_id in grid_candidates_for_point(&polygon_index.grid, point) {
        let polygon = &polygon_index.polygons[polygon_index_id];
        if !bounds_contains_point(polygon.bounds, point) {
            continue;
        }
        if point_in_polygon_points(point, &polygon.points) {
            is_inside = true;
            break;
        }
    }

    sample_cache.insert(key, is_inside);
    is_inside
}

fn grid_candidates_for_point(grid: &UniformGridIndex, point: [f32; 2]) -> &[usize] {
    let x = cell_coord(point[0], grid.min[0], grid.cell_size[0], grid.resolution);
    let y = cell_coord(point[1], grid.min[1], grid.cell_size[1], grid.resolution);
    match grid.cells.get(&(x, y)) {
        Some(indices) => indices.as_slice(),
        None => &[],
    }
}

fn bounds_contains_point(bounds: SegmentBounds, point: [f32; 2]) -> bool {
    point[0] >= bounds.min[0] - AREA_EPSILON
        && point[0] <= bounds.max[0] + AREA_EPSILON
        && point[1] >= bounds.min[1] - AREA_EPSILON
        && point[1] <= bounds.max[1] + AREA_EPSILON
}

fn point_in_polygon_points(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let mut previous = polygon.last().copied().expect("polygon is non-empty");
    for &current in polygon {
        let intersects = ((current[1] > point[1]) != (previous[1] > point[1]))
            && (point[0]
                < (previous[0] - current[0]) * (point[1] - current[1])
                    / (previous[1] - current[1])
                    + current[0]);
        if intersects {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn point_in_polygon(
    point: [f32; 2],
    polygon: &[QPoint],
    point_positions: &HashMap<QPoint, [f32; 2]>,
) -> bool {
    let mut inside = false;
    let mut previous = point_positions[polygon.last().expect("polygon is non-empty")];
    for vertex in polygon {
        let current = point_positions[vertex];
        let intersects = ((current[1] > point[1]) != (previous[1] > point[1]))
            && (point[0]
                < (previous[0] - current[0]) * (point[1] - current[1])
                    / (previous[1] - current[1])
                    + current[0]);
        if intersects {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

#[cfg(test)]
fn point_in_polygons_bruteforce(point: [f32; 2], polygons: &[Polygon]) -> bool {
    for polygon in polygons {
        if point_in_polygon_points(point, &polygon.points) {
            return true;
        }
    }
    false
}

fn polygon_area(points: &[QPoint], point_positions: &HashMap<QPoint, [f32; 2]>) -> f32 {
    let mut area = 0.0;
    for idx in 0..points.len() {
        let current = point_positions[&points[idx]];
        let next = point_positions[&points[(idx + 1) % points.len()]];
        area += current[0] * next[1] - next[0] * current[1];
    }
    area * 0.5
}

fn build_paths_from_segments(segments: &[SegmentInfo]) -> Vec<Vec<QPoint>> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for segment in segments {
        let Some(canonical) = canonical_segment(segment.a, segment.b) else {
            continue;
        };
        if seen.insert(canonical) {
            unique.push(segment_from_canonical(canonical));
        }
    }
    unique.sort_by_key(|segment| (segment.a, segment.b));

    let mut adjacency: HashMap<QPoint, Vec<(usize, QPoint)>> = HashMap::new();
    for (idx, segment) in unique.iter().enumerate() {
        adjacency
            .entry(segment.a)
            .or_default()
            .push((idx, segment.b));
        adjacency
            .entry(segment.b)
            .or_default()
            .push((idx, segment.a));
    }

    let mut visited = vec![false; unique.len()];
    let mut paths = Vec::new();

    let mut start_vertices = adjacency.keys().copied().collect::<Vec<_>>();
    start_vertices.sort();
    start_vertices
        .sort_by_key(|point| adjacency.get(point).map(|items| items.len()).unwrap_or(0) == 2);

    for start in start_vertices {
        let Some(neighbors) = adjacency.get(&start) else {
            continue;
        };
        if neighbors.len() == 2 {
            continue;
        }

        for &(segment_idx, _) in neighbors {
            if visited[segment_idx] {
                continue;
            }
            let path = trace_path(start, segment_idx, &unique, &adjacency, &mut visited);
            if path.len() >= 2 {
                paths.push(path);
            }
        }
    }

    for (idx, segment) in unique.iter().enumerate() {
        if visited[idx] {
            continue;
        }
        let mut path = trace_path(segment.a, idx, &unique, &adjacency, &mut visited);
        if path.first() != path.last() {
            path.push(path[0]);
        }
        if path.len() >= 3 {
            paths.push(path);
        }
    }

    paths
}

fn trace_path(
    start_vertex: QPoint,
    start_segment_idx: usize,
    segments: &[SegmentInfo],
    adjacency: &HashMap<QPoint, Vec<(usize, QPoint)>>,
    visited: &mut [bool],
) -> Vec<QPoint> {
    let mut path = vec![start_vertex];
    let mut current_vertex = start_vertex;
    let mut current_segment_idx = start_segment_idx;

    loop {
        if visited[current_segment_idx] {
            break;
        }
        visited[current_segment_idx] = true;

        let segment = segments[current_segment_idx];
        let next_vertex = if segment.a == current_vertex {
            segment.b
        } else {
            segment.a
        };
        path.push(next_vertex);
        current_vertex = next_vertex;

        // Stop chains at junctions/endpoints. Only degree-2 vertices can be traversed
        // through without changing the represented segment geometry.
        let Some(current_neighbors) = adjacency.get(&current_vertex) else {
            break;
        };
        if current_neighbors.len() != 2 {
            break;
        }

        let Some(next_segment_idx) = current_neighbors
            .iter()
            .map(|(idx, _)| *idx)
            .find(|idx| !visited[*idx])
        else {
            break;
        };
        current_segment_idx = next_segment_idx;
    }

    path
}

fn draw_edges_svg(prepared: &PreparedDrawing, width: u32, height: u32) -> Document {
    let mut document = Document::new().set("viewBox", (0, 0, width, height));

    for group in &prepared.groups {
        if group.paths.is_empty() {
            continue;
        }

        let color = format!(
            "rgb({}, {}, {})",
            group.line_color[0], group.line_color[1], group.line_color[2]
        );

        for path in &group.paths {
            let Some(data) = svg_path_data(path, width, height) else {
                continue;
            };

            let svg_path = SvgPath::new()
                .set("fill", "none")
                .set("stroke", color.clone())
                .set("stroke-width", group.line_width)
                .set("stroke-linecap", "round")
                .set("stroke-linejoin", "round")
                .set("d", data);

            document = document.add(svg_path);
        }
    }

    document
}

fn svg_path_data(path: &[QPoint], width: u32, height: u32) -> Option<Data> {
    let first = path.first()?;
    let first_point = to_canvas_point(*first, width, height);
    let mut data = Data::new().move_to((first_point[0] as f64, first_point[1] as f64));

    for point in &path[1..] {
        let canvas = to_canvas_point(*point, width, height);
        data = data.line_to((canvas[0] as f64, canvas[1] as f64));
    }

    if path.len() >= 3 && path.first() == path.last() {
        data = data.close();
    }

    Some(data)
}

fn draw_edges_raster(
    prepared: &PreparedDrawing,
    width: u32,
    height: u32,
) -> Result<Pixmap, BoxError> {
    let mut pixmap =
        Pixmap::new(width, height).ok_or_else(|| "failed to allocate raster pixmap".to_string())?;

    for group in &prepared.groups {
        if group.paths.is_empty() {
            continue;
        }

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            group.line_color[0],
            group.line_color[1],
            group.line_color[2],
            group.line_color[3],
        ));

        let stroke = Stroke {
            width: group.line_width.max(0.5),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };

        for path in &group.paths {
            let Some(sk_path) = build_skia_path(path, width, height) else {
                continue;
            };
            pixmap.stroke_path(&sk_path, &paint, &stroke, Transform::identity(), None);
        }
    }

    Ok(pixmap)
}

fn build_skia_path(path: &[QPoint], width: u32, height: u32) -> Option<tiny_skia::Path> {
    let first = *path.first()?;
    let first_point = to_canvas_point(first, width, height);
    let mut builder = PathBuilder::new();
    builder.move_to(first_point[0], first_point[1]);

    for point in &path[1..] {
        let canvas = to_canvas_point(*point, width, height);
        builder.line_to(canvas[0], canvas[1]);
    }

    if path.len() >= 3 && path.first() == path.last() {
        builder.close();
    }

    builder.finish()
}

fn save_image(pixmap: &Pixmap, image_path: &Path) -> Result<(), BoxError> {
    pixmap.save_png(image_path)?;
    Ok(())
}

fn save_svg(document: &Document, image_path: &Path) -> Result<(), BoxError> {
    svg::save(image_path, document)?;
    Ok(())
}

fn quantize_point(point: [f32; 2]) -> QPoint {
    QPoint {
        x: (point[0] * QUANTIZE_SCALE).round() as i64,
        y: (point[1] * QUANTIZE_SCALE).round() as i64,
    }
}

impl Edges {
    fn fallback_line_width(&self) -> f32 {
        self.line_width.unwrap_or(1.0)
    }

    fn fallback_line_color(&self) -> [u8; 4] {
        self.line_color.unwrap_or([0, 0, 0, 255])
    }

    fn effective_outline_width(&self) -> f32 {
        self.outline_width
            .unwrap_or_else(|| self.fallback_line_width())
    }

    fn effective_internal_width(&self) -> f32 {
        self.internal_width
            .unwrap_or_else(|| self.fallback_line_width())
    }

    fn effective_outline_color(&self) -> [u8; 4] {
        self.outline_color
            .unwrap_or_else(|| self.fallback_line_color())
    }

    fn effective_internal_color(&self) -> [u8; 4] {
        self.internal_color
            .unwrap_or_else(|| self.fallback_line_color())
    }

    fn effective_draw_outline(&self) -> bool {
        match self.hide_internal {
            Some(_) => true,
            None => self.draw_outline,
        }
    }

    fn effective_draw_internal(&self) -> bool {
        match self.hide_internal {
            Some(hide_internal) => !hide_internal,
            None => self.draw_internal,
        }
    }
}

fn canonical_segment(a: QPoint, b: QPoint) -> Option<CanonicalSegment> {
    if a == b {
        return None;
    }
    Some(if a <= b {
        CanonicalSegment { start: a, end: b }
    } else {
        CanonicalSegment { start: b, end: a }
    })
}

fn segment_from_canonical(segment: CanonicalSegment) -> SegmentInfo {
    SegmentInfo {
        a: segment.start,
        b: segment.end,
    }
}

fn to_canvas_point(point: QPoint, width: u32, height: u32) -> [f32; 2] {
    let uv = [
        point.x as f32 / QUANTIZE_SCALE,
        point.y as f32 / QUANTIZE_SCALE,
    ];
    [uv[0] * width as f32, (1.0 - uv[1]) * height as f32]
}

fn compact_payload_from_buffers(
    group_line_offsets: Vec<usize>,
    line_points: Vec<f32>,
    group_internal_widths: Vec<f32>,
    group_outline_widths: Vec<f32>,
    group_internal_colors: Vec<u8>,
    group_outline_colors: Vec<u8>,
    group_draw_outline: Vec<bool>,
    group_draw_internal: Vec<bool>,
    polygon_offsets: Vec<usize>,
    polygon_points: Vec<f32>,
    warning_enabled: bool,
    padding_pixels: f32,
    warning_width: f32,
    warning_color: Vec<u8>,
) -> Result<CompactPayload, BoxError> {
    let group_count = group_internal_widths.len();
    if group_line_offsets.len() != group_count + 1
        || group_outline_widths.len() != group_count
        || group_draw_outline.len() != group_count
        || group_draw_internal.len() != group_count
        || group_internal_colors.len() != group_count * 4
        || group_outline_colors.len() != group_count * 4
    {
        return Err("Invalid group buffer lengths".into());
    }
    if line_points.len() % 4 != 0 {
        return Err("line_points length must be divisible by 4".into());
    }
    if polygon_offsets.is_empty() || polygon_offsets[0] != 0 || polygon_points.len() % 2 != 0 {
        return Err("Invalid polygon buffers".into());
    }
    if warning_color.len() != 4 {
        return Err("warning_color must have 4 items".into());
    }

    let mut styles = Vec::with_capacity(group_count);
    let mut point_positions = HashMap::new();
    let mut original_segments = Vec::<CanonicalSegment>::new();
    let mut original_segment_set = HashSet::<CanonicalSegment>::new();
    let mut group_segments = Vec::with_capacity(group_count);

    for group_index in 0..group_count {
        styles.push(DrawStyle {
            internal_width: group_internal_widths[group_index].max(0.0),
            outline_width: group_outline_widths[group_index].max(0.0),
            internal_color: [
                group_internal_colors[group_index * 4],
                group_internal_colors[group_index * 4 + 1],
                group_internal_colors[group_index * 4 + 2],
                group_internal_colors[group_index * 4 + 3],
            ],
            outline_color: [
                group_outline_colors[group_index * 4],
                group_outline_colors[group_index * 4 + 1],
                group_outline_colors[group_index * 4 + 2],
                group_outline_colors[group_index * 4 + 3],
            ],
            draw_outline: group_draw_outline[group_index],
            draw_internal: group_draw_internal[group_index],
        });

        let start = group_line_offsets[group_index];
        let end = group_line_offsets[group_index + 1];
        let mut group_seen = HashSet::new();
        let mut segments = Vec::new();
        for line_index in start..end {
            let base = line_index * 4;
            let uv1 = [line_points[base], line_points[base + 1]];
            let uv2 = [line_points[base + 2], line_points[base + 3]];
            let q1 = quantize_point(uv1);
            let q2 = quantize_point(uv2);
            point_positions.entry(q1).or_insert(uv1);
            point_positions.entry(q2).or_insert(uv2);
            let Some(canonical) = canonical_segment(q1, q2) else {
                continue;
            };
            if original_segment_set.insert(canonical) {
                original_segments.push(canonical);
            }
            if group_seen.insert(canonical) {
                segments.push(canonical);
            }
        }
        segments.sort_by_key(|segment| (segment.start, segment.end));
        group_segments.push(segments);
    }
    original_segments.sort_by_key(|segment| (segment.start, segment.end));

    let polygon_count = polygon_offsets.len() - 1;
    let mut polygons = Vec::with_capacity(polygon_count);
    for polygon_index in 0..polygon_count {
        let start = polygon_offsets[polygon_index];
        let end = polygon_offsets[polygon_index + 1];
        let mut points = Vec::with_capacity(end.saturating_sub(start));
        for point_index in start..end {
            let base = point_index * 2;
            points.push([polygon_points[base], polygon_points[base + 1]]);
        }
        polygons.push(Polygon { points });
    }

    let padding_warning = if warning_enabled {
        Some(PaddingWarningConfig {
            enabled: true,
            padding_pixels,
            warning_width,
            warning_color: [
                warning_color[0],
                warning_color[1],
                warning_color[2],
                warning_color[3],
            ],
        })
    } else {
        None
    };

    Ok(CompactPayload {
        styles,
        arrangement_input_segments: original_segments,
        arrangement_input_group_segments: group_segments,
        point_positions,
        polygons,
        padding_warning,
    })
}

#[pyfunction(name = "draw_edges")]
fn draw_edges_py(image_path: &str, width: u32, height: u32, edges_json: &str) -> PyResult<()> {
    draw_to_path(Path::new(image_path), width, height, edges_json)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

#[allow(clippy::too_many_arguments)]
#[pyfunction(name = "draw_edges_buffered")]
fn draw_edges_buffered_py(
    image_path: &str,
    width: u32,
    height: u32,
    group_line_offsets: Vec<usize>,
    line_points: Vec<f32>,
    group_internal_widths: Vec<f32>,
    group_outline_widths: Vec<f32>,
    group_internal_colors: Vec<u8>,
    group_outline_colors: Vec<u8>,
    group_draw_outline: Vec<bool>,
    group_draw_internal: Vec<bool>,
    polygon_offsets: Vec<usize>,
    polygon_points: Vec<f32>,
    warning_enabled: bool,
    padding_pixels: f32,
    warning_width: f32,
    warning_color: Vec<u8>,
) -> PyResult<()> {
    let payload = compact_payload_from_buffers(
        group_line_offsets,
        line_points,
        group_internal_widths,
        group_outline_widths,
        group_internal_colors,
        group_outline_colors,
        group_draw_outline,
        group_draw_internal,
        polygon_offsets,
        polygon_points,
        warning_enabled,
        padding_pixels,
        warning_width,
        warning_color,
    )
    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
    let prepare_started_at = Instant::now();
    let prepared = prepare_drawing_from_compact(&payload, width, height);
    log_profile("prepare_total", prepare_started_at);

    if Path::new(image_path).extension().and_then(|s| s.to_str()) == Some("svg") {
        let render_started_at = Instant::now();
        let document = draw_edges_svg(&prepared, width, height);
        log_profile("render_svg", render_started_at);
        save_svg(&document, Path::new(image_path))
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    } else {
        let render_started_at = Instant::now();
        let pixmap = draw_edges_raster(&prepared, width, height)
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
        log_profile("render_raster", render_started_at);
        save_image(&pixmap, Path::new(image_path))
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }
}

#[pymodule(name = "_edge_drawer")]
fn _edge_drawer(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(draw_edges_py, module)?)?;
    module.add_function(wrap_pyfunction!(draw_edges_buffered_py, module)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    const VALID_JSON: &str = r#"
    [
      {
        "internal_color": [255, 0, 0, 255],
        "outline_color": [0, 255, 0, 255],
        "internal_width": 2.0,
        "outline_width": 4.0,
        "draw_outline": true,
        "draw_internal": false,
        "lines": [
          {"uv1": [0.1, 0.1], "uv2": [0.8, 0.8]},
          {"uv1": [0.2, 0.8], "uv2": [0.8, 0.2]}
        ]
      }
    ]
    "#;

    fn square_with_diagonal_json(draw_outline: bool, draw_internal: bool) -> String {
        format!(
            r#"
            [
              {{
                "internal_color": [255, 255, 255, 255],
                "outline_color": [255, 255, 255, 255],
                "internal_width": 3.0,
                "outline_width": 6.0,
                "draw_outline": {},
                "draw_internal": {},
                "lines": [
                  {{"uv1": [0.1, 0.1], "uv2": [0.9, 0.1]}},
                  {{"uv1": [0.9, 0.1], "uv2": [0.9, 0.9]}},
                  {{"uv1": [0.9, 0.9], "uv2": [0.1, 0.9]}},
                  {{"uv1": [0.1, 0.9], "uv2": [0.1, 0.1]}},
                  {{"uv1": [0.1, 0.1], "uv2": [0.9, 0.9]}}
                ]
              }}
            ]
            "#,
            if draw_outline { "true" } else { "false" },
            if draw_internal { "true" } else { "false" }
        )
    }

    fn payload_with_warning_json(padding_pixels: f32) -> String {
        format!(
            r#"{{
                "edges": [
                    {{
                        "outline_color": [255, 255, 255, 255],
                        "outline_width": 4.0,
                        "draw_outline": true,
                        "draw_internal": false,
                        "lines": [
                            {{"uv1": [0.01, 0.2], "uv2": [0.01, 0.8]}}
                        ]
                    }}
                ],
                "polygons": [
                    {{
                        "points": [[0.01, 0.2], [0.01, 0.8], [0.2, 0.8], [0.2, 0.2]]
                    }}
                ],
                "padding_warning": {{
                    "enabled": true,
                    "padding_pixels": {},
                    "warning_width": 6.0,
                    "warning_color": [255, 64, 64, 255]
                }}
            }}"#,
            padding_pixels
        )
    }

    fn two_outline_islands_json() -> String {
        r#"
        [
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.1, 0.2], "uv2": [0.1, 0.8]}
            ]
          },
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.12, 0.2], "uv2": [0.12, 0.8]}
            ]
          }
        ]
        "#
        .to_string()
    }

    fn diagonal_segment() -> CanonicalSegment {
        canonical_segment(quantize_point([0.1, 0.1]), quantize_point([0.9, 0.9])).unwrap()
    }

    fn near_left_border_segment() -> CanonicalSegment {
        canonical_segment(quantize_point([0.01, 0.2]), quantize_point([0.01, 0.8])).unwrap()
    }

    fn island_left_segment() -> CanonicalSegment {
        canonical_segment(quantize_point([0.1, 0.2]), quantize_point([0.1, 0.8])).unwrap()
    }

    fn island_right_segment() -> CanonicalSegment {
        canonical_segment(quantize_point([0.12, 0.2]), quantize_point([0.12, 0.8])).unwrap()
    }

    fn overlapping_rectangles_json() -> String {
        r#"
        [
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.1, 0.1], "uv2": [0.9, 0.1]},
              {"uv1": [0.9, 0.1], "uv2": [0.9, 0.9]},
              {"uv1": [0.9, 0.9], "uv2": [0.1, 0.9]},
              {"uv1": [0.1, 0.9], "uv2": [0.1, 0.1]}
            ]
          },
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.1, 0.1], "uv2": [0.9, 0.1]},
              {"uv1": [0.9, 0.1], "uv2": [0.9, 0.9]},
              {"uv1": [0.9, 0.9], "uv2": [0.1, 0.9]},
              {"uv1": [0.1, 0.9], "uv2": [0.1, 0.1]}
            ]
          }
        ]
        "#
        .to_string()
    }

    fn offset_rectangles_json() -> String {
        r#"
        [
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.1, 0.2], "uv2": [0.5, 0.2]},
              {"uv1": [0.5, 0.2], "uv2": [0.5, 0.8]},
              {"uv1": [0.5, 0.8], "uv2": [0.1, 0.8]},
              {"uv1": [0.1, 0.8], "uv2": [0.1, 0.2]}
            ]
          },
          {
            "outline_color": [255, 255, 255, 255],
            "outline_width": 4.0,
            "draw_outline": true,
            "draw_internal": false,
            "lines": [
              {"uv1": [0.3, 0.2], "uv2": [0.7, 0.2]},
              {"uv1": [0.7, 0.2], "uv2": [0.7, 0.8]},
              {"uv1": [0.7, 0.8], "uv2": [0.3, 0.8]},
              {"uv1": [0.3, 0.8], "uv2": [0.3, 0.2]}
            ]
          }
        ]
        "#
        .to_string()
    }

    fn overlapping_polygons() -> Vec<Polygon> {
        vec![
            Polygon {
                points: vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
            },
            Polygon {
                points: vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
            },
        ]
    }

    #[test]
    fn test_parse_edges_json_valid_data() {
        let edges = parse_edges_json(VALID_JSON).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].effective_internal_width(), 2.0);
        assert_eq!(edges[0].effective_outline_width(), 4.0);
        assert_eq!(edges[0].effective_internal_color(), [255, 0, 0, 255]);
        assert_eq!(edges[0].effective_outline_color(), [0, 255, 0, 255]);
        assert!(edges[0].effective_draw_outline());
        assert!(!edges[0].effective_draw_internal());
        assert_eq!(edges[0].lines.len(), 2);
        assert_eq!(edges[0].lines[0].uv1, [0.1, 0.1]);
        assert_eq!(edges[0].lines[0].uv2, [0.8, 0.8]);
    }

    #[test]
    fn test_parse_drawer_payload_object_form() {
        let payload = parse_drawer_payload(&payload_with_warning_json(8.0)).unwrap();
        assert_eq!(payload.edges.len(), 1);
        assert_eq!(payload.polygons.len(), 1);
        assert!(payload.padding_warning.is_some());
        let warning = payload.padding_warning.unwrap();
        assert!(warning.enabled);
        assert_eq!(warning.padding_pixels, 8.0);
        assert_eq!(warning.warning_width, 6.0);
        assert_eq!(warning.warning_color, [255, 64, 64, 255]);
    }

    #[test]
    fn test_parse_edges_json_defaults_draw_modes() {
        let edges = parse_edges_json(
            r#"[{"line_color":[1,2,3,255],"line_width":1.0,"lines":[{"uv1":[0.0,0.0],"uv2":[1.0,0.0]}]}]"#,
        )
        .unwrap();
        assert!(edges[0].effective_draw_outline());
        assert!(edges[0].effective_draw_internal());
        assert_eq!(edges[0].effective_internal_width(), 1.0);
        assert_eq!(edges[0].effective_outline_width(), 1.0);
        assert_eq!(edges[0].effective_internal_color(), [1, 2, 3, 255]);
        assert_eq!(edges[0].effective_outline_color(), [1, 2, 3, 255]);
    }

    #[test]
    fn test_parse_edges_json_hide_internal_compatibility() {
        let edges = parse_edges_json(
            r#"[{"line_color":[1,2,3,255],"line_width":1.0,"hide_internal":true,"lines":[{"uv1":[0.0,0.0],"uv2":[1.0,0.0]}]}]"#,
        )
        .unwrap();
        assert!(edges[0].effective_draw_outline());
        assert!(!edges[0].effective_draw_internal());
    }

    #[test]
    fn test_parse_edges_json_separate_widths_override_legacy_width() {
        let edges = parse_edges_json(
            r#"[{"line_color":[1,2,3,255],"line_width":1.0,"internal_width":2.0,"outline_width":5.0,"lines":[{"uv1":[0.0,0.0],"uv2":[1.0,0.0]}]}]"#,
        )
        .unwrap();
        assert_eq!(edges[0].effective_internal_width(), 2.0);
        assert_eq!(edges[0].effective_outline_width(), 5.0);
    }

    #[test]
    fn test_parse_edges_json_separate_colors_override_legacy_color() {
        let edges = parse_edges_json(
            r#"[{"line_color":[1,2,3,255],"internal_color":[4,5,6,255],"outline_color":[7,8,9,255],"line_width":1.0,"lines":[{"uv1":[0.0,0.0],"uv2":[1.0,0.0]}]}]"#,
        )
        .unwrap();
        assert_eq!(edges[0].effective_internal_color(), [4, 5, 6, 255]);
        assert_eq!(edges[0].effective_outline_color(), [7, 8, 9, 255]);
    }

    #[test]
    fn test_parse_edges_json_invalid_json() {
        let invalid_json = r#"[{"line_width": 1.0,]"#;
        let err = parse_edges_json(invalid_json).unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_parse_edges_json_invalid_shape() {
        let invalid_shape = r#"
        [
          {"uv1": [0.1, 0.1], "uv2": [0.2, 0.2]}
        ]
        "#;
        let err = parse_edges_json(invalid_shape).unwrap_err().to_string();
        assert!(err.contains("missing field"));
    }

    #[test]
    fn test_hide_internal_removes_diagonal() {
        let edges = parse_edges_json(&square_with_diagonal_json(true, false)).unwrap();
        let visible = classify_visible_segments(&edges);
        assert!(!visible.contains(&diagonal_segment()));
    }

    #[test]
    fn test_outline_and_internal_keep_diagonal() {
        let edges = parse_edges_json(&square_with_diagonal_json(true, true)).unwrap();
        let visible = classify_visible_segments(&edges);
        assert!(visible.contains(&diagonal_segment()));
    }

    #[test]
    fn test_internal_classification_is_stable_across_runs() {
        let edges = parse_edges_json(&square_with_diagonal_json(true, false)).unwrap();
        let expected = classify_visible_segments(&edges);

        for _ in 0..32 {
            let visible = classify_visible_segments(&edges);
            assert_eq!(visible, expected);
        }
    }

    #[test]
    fn test_internal_only_removes_outline_segments() {
        let edges = parse_edges_json(&square_with_diagonal_json(false, true)).unwrap();
        let visible = classify_visible_segments(&edges);
        assert!(visible.contains(&diagonal_segment()));
        assert_eq!(visible.len(), 1);
    }

    #[test]
    fn test_prepared_groups_split_internal_and_outline_widths() {
        let edges = parse_edges_json(&square_with_diagonal_json(true, true)).unwrap();
        let prepared = prepare_drawing(&edges, &[], 128, 128, None);
        assert_eq!(prepared.groups.len(), 2);
        assert_eq!(prepared.groups[0].line_width, 3.0);
        assert_eq!(prepared.groups[1].line_width, 6.0);
    }

    #[test]
    fn test_padding_warning_marks_border_close_outline() {
        let edges = parse_edges_json(
            r#"[{"outline_color":[255,255,255,255],"outline_width":4.0,"draw_outline":true,"draw_internal":false,"lines":[{"uv1":[0.01,0.2],"uv2":[0.01,0.8]}]}]"#,
        )
        .unwrap();
        let warning_segments = classify_warning_segments(&edges, 100, 100, 2.0);
        assert!(warning_segments.contains(&near_left_border_segment()));
    }

    #[test]
    fn test_padding_warning_marks_close_outline_pairs() {
        let edges = parse_edges_json(&two_outline_islands_json()).unwrap();
        let warning_segments = classify_warning_segments(&edges, 100, 100, 3.0);
        assert!(warning_segments.contains(&island_left_segment()));
        assert!(warning_segments.contains(&island_right_segment()));
    }

    #[test]
    fn test_disabling_both_modes_removes_group() {
        let edges = parse_edges_json(&square_with_diagonal_json(false, false)).unwrap();
        let visible = classify_visible_segments(&edges);
        assert!(visible.is_empty());
    }

    #[test]
    fn test_identical_overlapping_rectangles_keep_only_outer_outline() {
        let edges = parse_edges_json(&overlapping_rectangles_json()).unwrap();
        let visible = classify_visible_segments(&edges);
        assert_eq!(visible.len(), 4);
    }

    #[test]
    fn test_offset_rectangles_split_overlap_into_union_outline() {
        let edges = parse_edges_json(&offset_rectangles_json()).unwrap();
        let visible = classify_visible_segments(&edges);
        assert!(visible.contains(&canonical_segment(quantize_point([0.1, 0.2]), quantize_point([0.3, 0.2])).unwrap()));
        assert!(visible.contains(&canonical_segment(quantize_point([0.3, 0.2]), quantize_point([0.5, 0.2])).unwrap()));
        assert!(visible.contains(&canonical_segment(quantize_point([0.5, 0.2]), quantize_point([0.7, 0.2])).unwrap()));
        assert!(!visible.contains(&canonical_segment(quantize_point([0.3, 0.2]), quantize_point([0.3, 0.8])).unwrap()));
        assert!(!visible.contains(&canonical_segment(quantize_point([0.5, 0.2]), quantize_point([0.5, 0.8])).unwrap()));
    }

    #[test]
    fn test_point_in_polygons_treats_overlaps_as_union() {
        let polygons = overlapping_polygons();
        let polygon_index = build_polygon_index(&polygons);
        let mut sample_cache = HashMap::new();
        assert!(point_in_polygons([0.5, 0.5], &polygon_index, &mut sample_cache));
        assert!(!point_in_polygons([0.95, 0.95], &polygon_index, &mut sample_cache));
        assert_eq!(
            point_in_polygons_bruteforce([0.5, 0.5], &polygons),
            point_in_polygons([0.5, 0.5], &polygon_index, &mut sample_cache)
        );
    }

    #[test]
    fn test_build_paths_stops_at_junctions() {
        let a = quantize_point([0.1, 0.1]);
        let b = quantize_point([0.5, 0.5]);
        let c = quantize_point([0.8, 0.8]);
        let d = quantize_point([0.8, 0.2]);
        let segments = vec![
            SegmentInfo { a, b },
            SegmentInfo { a: b, b: c },
            SegmentInfo { a: b, b: d },
        ];

        let paths = build_paths_from_segments(&segments);
        assert_eq!(paths.len(), 3);
        assert!(paths.iter().any(|path| path == &vec![a, b] || path == &vec![b, a]));
        assert!(paths.iter().any(|path| path == &vec![b, c] || path == &vec![c, b]));
        assert!(paths.iter().any(|path| path == &vec![b, d] || path == &vec![d, b]));
    }

    #[test]
    fn test_draw_png_to_path() {
        let dir = tempdir().unwrap();
        let image_path = dir.path().join("edge.png");

        draw_to_path(image_path.as_path(), 128, 128, VALID_JSON).unwrap();

        assert!(image_path.exists());
        let bytes = fs::read(&image_path).unwrap();
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_draw_svg_to_path() {
        let dir = tempdir().unwrap();
        let image_path = dir.path().join("edge.svg");

        draw_to_path(
            image_path.as_path(),
            128,
            128,
            &square_with_diagonal_json(true, false),
        )
        .unwrap();

        let contents = fs::read_to_string(&image_path).unwrap();
        assert!(contents.contains("<svg"));
        assert!(contents.contains("stroke-linejoin=\"round\""));
        assert!(contents.contains("stroke-width=\"6\""));
    }

    #[test]
    fn test_load_edges_input_from_file() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("edges.json");
        fs::write(&json_path, VALID_JSON).unwrap();

        let payload = load_edges_input(json_path.to_str().unwrap()).unwrap();
        assert_eq!(payload.edges.len(), 1);
    }
}
