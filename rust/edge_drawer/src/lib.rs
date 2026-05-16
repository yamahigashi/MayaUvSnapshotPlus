use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use i_overlay::core::fill_rule::FillRule as OverlayFillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::overlay::FloatOverlay;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::Deserialize;
use svg::node::element::path::Data;
use svg::node::element::Path as SvgPath;
use svg::Document;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Stroke,
    Transform,
};

type BoxError = Box<dyn Error + Send + Sync>;
type FillContour = Vec<QPoint>;
type FillShape = Vec<FillContour>;
type OverlayPoint = [f64; 2];
type OverlayContour = Vec<OverlayPoint>;
type OverlayShape = Vec<OverlayContour>;
type OverlayShapes = Vec<OverlayShape>;

const QUANTIZE_SCALE: f32 = 1_000.0;
const SAMPLE_EPSILON: f32 = 1e-4;
const AREA_EPSILON: f32 = 1e-8;
const DEFAULT_WARNING_COLOR: [u8; 4] = [255, 64, 64, 255];
const DEFAULT_WARNING_WIDTH: f32 = 4.0;
const DEFAULT_ISLAND_FILL_OPACITY: f32 = 0.25;
const DEFAULT_ISLAND_FILL_PADDING_PIXELS: f32 = 0.0;
const ISLAND_FILL_PALETTE: [[u8; 3]; 12] = [
    [94, 176, 255],
    [255, 173, 77],
    [117, 209, 140],
    [245, 118, 136],
    [171, 132, 255],
    [84, 203, 196],
    [235, 215, 93],
    [255, 139, 213],
    [138, 183, 86],
    [111, 136, 214],
    [227, 147, 103],
    [103, 190, 231],
];

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
    pub island_fill: Option<IslandFillConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct IslandFillConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_island_fill_opacity")]
    opacity: f32,
    #[serde(default = "default_island_fill_padding_pixels")]
    padding_pixels: f32,
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
    fills: Vec<PreparedFill>,
    groups: Vec<PreparedGroup>,
}

#[derive(Clone, Debug)]
struct PreparedFill {
    fill_color: [u8; 4],
    padding_pixels: f32,
    shapes: Vec<FillShape>,
}

#[derive(Clone, Copy, Debug)]
struct OverlayBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
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
    point_positions: PointPositionIndex,
    group_segments: Vec<Vec<CanonicalSegment>>,
}

#[derive(Debug)]
struct ArrangementInputs {
    point_positions: Arc<HashMap<QPoint, [f32; 2]>>,
    original_segments: Vec<CanonicalSegment>,
    group_segments: Vec<Vec<CanonicalSegment>>,
    group_segment_indices: Vec<Vec<usize>>,
}

#[derive(Clone, Debug)]
struct CompactPayload {
    styles: Vec<DrawStyle>,
    arrangement_input_segments: Vec<CanonicalSegment>,
    arrangement_input_group_segments: Vec<Vec<CanonicalSegment>>,
    arrangement_input_group_segment_indices: Vec<Vec<usize>>,
    point_positions: Arc<HashMap<QPoint, [f32; 2]>>,
    polygons: Vec<Polygon>,
    padding_warning: Option<PaddingWarningConfig>,
    island_fill: Option<IslandFillConfig>,
}

#[derive(Clone, Debug)]
struct PointPositionIndex {
    base: Arc<HashMap<QPoint, [f32; 2]>>,
    extra: HashMap<QPoint, [f32; 2]>,
}

#[derive(Clone, Copy, Debug)]
struct SegmentBounds {
    min: [f32; 2],
    max: [f32; 2],
}

#[derive(Clone, Debug)]
struct UniformGridIndex {
    cells: HashMap<(i32, i32), Vec<usize>>,
}

#[derive(Clone, Debug)]
struct IndexedPolygon<'a> {
    points: &'a [[f32; 2]],
    bounds: SegmentBounds,
}

#[derive(Clone, Debug)]
struct DenseGridIndex {
    min: [f32; 2],
    cell_size: [f32; 2],
    resolution: u32,
    cell_starts: Vec<usize>,
    indices: Vec<usize>,
}

#[derive(Clone, Debug)]
struct ArrangementGridIndex {
    dense: DenseGridIndex,
    segment_cell_starts: Vec<usize>,
    segment_cells: Vec<usize>,
}

#[derive(Clone, Debug)]
struct PolygonIndex<'a> {
    polygons: Vec<IndexedPolygon<'a>>,
    grid: DenseGridIndex,
}

#[derive(Default)]
struct ClassificationStats {
    sample_queries: usize,
    candidate_polygons: usize,
    bounds_checks: usize,
    point_in_polygon_tests: usize,
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

fn pair_profile_enabled() -> bool {
    std::env::var("EDGE_DRAWER_PAIR_PROFILE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn split_profile_enabled() -> bool {
    std::env::var("EDGE_DRAWER_SPLIT_PROFILE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn arrangement_detail_profile_enabled() -> bool {
    std::env::var("EDGE_DRAWER_ARRANGEMENT_PROFILE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn classification_detail_profile_enabled() -> bool {
    std::env::var("EDGE_DRAWER_CLASSIFICATION_PROFILE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn log_profile(label: &str, started_at: Instant) {
    if profile_enabled() {
        eprintln!(
            "edge_drawer: {} {:.4}s",
            label,
            started_at.elapsed().as_secs_f64()
        );
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

fn default_island_fill_opacity() -> f32 {
    DEFAULT_ISLAND_FILL_OPACITY
}

fn default_island_fill_padding_pixels() -> f32 {
    DEFAULT_ISLAND_FILL_PADDING_PIXELS
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
            island_fill: None,
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
        Some(value) if !value.is_null() => Some(serde_json::from_value(value)?),
        None => None,
        _ => None,
    };
    let island_fill = match payload_object.get("island_fill").cloned() {
        Some(value) if !value.is_null() => Some(serde_json::from_value(value)?),
        None => None,
        _ => None,
    };

    Ok(DrawerPayload {
        edges,
        polygons,
        padding_warning,
        island_fill,
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
        payload.island_fill.as_ref(),
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
            island_fill: None,
        },
    )
}

fn prepare_drawing(
    edges: &[Edges],
    polygons: &[Polygon],
    width: u32,
    height: u32,
    padding_warning: Option<&PaddingWarningConfig>,
    island_fill: Option<&IslandFillConfig>,
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
    let (internal_segments, outline_segments) = classify_segments(
        &arrangement.segments,
        &arrangement.point_positions,
        polygons,
    );
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

    let fill_started_at = Instant::now();
    let fills = build_island_fills(polygons, island_fill, width, height);
    log_profile("island_fill", fill_started_at);

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

    PreparedDrawing { fills, groups }
}

fn prepare_drawing_from_compact(
    payload: &CompactPayload,
    width: u32,
    height: u32,
) -> PreparedDrawing {
    let arrangement_started_at = Instant::now();
    let arrangement = build_segment_arrangement_from_parts(
        payload.arrangement_input_segments.clone(),
        Arc::clone(&payload.point_positions),
        &payload.arrangement_input_group_segments,
        &payload.arrangement_input_group_segment_indices,
    );
    log_profile("arrangement", arrangement_started_at);

    let classification_started_at = Instant::now();
    let (internal_segments, outline_segments) = classify_segments(
        &arrangement.segments,
        &arrangement.point_positions,
        &payload.polygons,
    );
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

    let fill_started_at = Instant::now();
    let fills = build_island_fills(
        &payload.polygons,
        payload.island_fill.as_ref(),
        width,
        height,
    );
    log_profile("island_fill", fill_started_at);

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

    PreparedDrawing { fills, groups }
}

fn point_position(point_positions: &PointPositionIndex, point: QPoint) -> [f32; 2] {
    point_positions
        .base
        .get(&point)
        .copied()
        .or_else(|| point_positions.extra.get(&point).copied())
        .expect("point position must exist")
}

fn insert_point_position(point_positions: &mut PointPositionIndex, point: QPoint, uv: [f32; 2]) {
    if point_positions.base.contains_key(&point) || point_positions.extra.contains_key(&point) {
        return;
    }
    point_positions.extra.insert(point, uv);
}

fn build_segment_arrangement(edges: &[Edges]) -> SegmentArrangement {
    let collect_started_at = Instant::now();
    let inputs = collect_arrangement_inputs(edges);
    if arrangement_detail_profile_enabled() {
        log_profile("arrangement_collect_inputs", collect_started_at);
    }
    build_segment_arrangement_from_parts(
        inputs.original_segments,
        inputs.point_positions,
        &inputs.group_segments,
        &inputs.group_segment_indices,
    )
}

fn build_segment_arrangement_from_parts(
    original_segments: Vec<CanonicalSegment>,
    base_point_positions: Arc<HashMap<QPoint, [f32; 2]>>,
    original_group_segments: &[Vec<CanonicalSegment>],
    original_group_segment_indices: &[Vec<usize>],
) -> SegmentArrangement {
    let detail_profile = arrangement_detail_profile_enabled();
    let mut point_positions = PointPositionIndex {
        base: base_point_positions,
        extra: HashMap::new(),
    };
    let setup_started_at = Instant::now();
    let segment_uvs = original_segments
        .iter()
        .map(|segment| {
            (
                point_position(&point_positions, segment.start),
                point_position(&point_positions, segment.end),
            )
        })
        .collect::<Vec<_>>();
    let mut split_points = vec![Vec::<QPoint>::new(); original_segments.len()];
    let segment_bounds = original_segments
        .iter()
        .zip(segment_uvs.iter())
        .map(|(_, (start, end))| SegmentBounds {
            min: [start[0].min(end[0]), start[1].min(end[1])],
            max: [start[0].max(end[0]), start[1].max(end[1])],
        })
        .collect::<Vec<_>>();
    if arrangement_detail_profile_enabled() {
        log_profile("arrangement_setup", setup_started_at);
    }

    let pair_pass_started_at = Instant::now();
    visit_candidate_pairs(&segment_bounds, 0.0, |left_index, right_index| {
        let left = original_segments[left_index];
        let right = original_segments[right_index];
        let (left_start, left_end) = segment_uvs[left_index];
        let (right_start, right_end) = segment_uvs[right_index];

        register_pair_splits(
            left,
            right,
            left_index,
            right_index,
            left_start,
            left_end,
            right_start,
            right_end,
            &mut point_positions,
            split_points.as_mut_slice(),
        );
    });
    if arrangement_detail_profile_enabled() {
        log_profile("arrangement_pairs", pair_pass_started_at);
    }

    log_split_point_profile(split_points.as_slice());
    normalize_split_points(split_points.as_mut_slice());

    if split_points.iter().all(|points| points.is_empty()) {
        return SegmentArrangement {
            segments: original_segments.clone(),
            point_positions,
            group_segments: original_group_segments.to_vec(),
        };
    }

    let finalize_started_at = Instant::now();
    let split_materialize_started_at = Instant::now();
    let mut split_segments_by_index = vec![None::<Vec<CanonicalSegment>>; original_segments.len()];
    for (segment_index, points) in split_points.iter().enumerate() {
        if points.is_empty() {
            continue;
        }

        let segment = original_segments[segment_index];
        let parts = split_segment(segment, Some(points.as_slice()), &point_positions);
        split_segments_by_index[segment_index] = Some(parts);
    }
    if detail_profile {
        log_profile(
            "arrangement_finalize_split_segments",
            split_materialize_started_at,
        );
    }

    let flatten_segments_started_at = Instant::now();
    let mut segments = Vec::with_capacity(original_segments.len());
    for (segment_index, &original) in original_segments.iter().enumerate() {
        if let Some(parts) = split_segments_by_index[segment_index].as_ref() {
            segments.extend(parts.iter().copied());
        } else {
            segments.push(original);
        }
    }
    dedup_sorted_segments_in_place(&mut segments);
    if detail_profile {
        log_profile(
            "arrangement_finalize_segments_rebuild",
            flatten_segments_started_at,
        );
    }

    let rebuild_groups_started_at = Instant::now();
    let mut group_segments = Vec::with_capacity(original_group_segments.len());
    for (original_group, group_indices) in original_group_segments
        .iter()
        .zip(original_group_segment_indices.iter())
    {
        if !group_indices
            .iter()
            .any(|&segment_index| split_segments_by_index[segment_index].is_some())
        {
            group_segments.push(original_group.clone());
            continue;
        }

        let mut parts = Vec::with_capacity(original_group.len());
        for &segment_index in group_indices {
            if let Some(split_parts) = split_segments_by_index[segment_index].as_ref() {
                parts.extend(split_parts.iter().copied());
            } else {
                parts.push(original_segments[segment_index]);
            }
        }
        dedup_sorted_segments_in_place(&mut parts);
        group_segments.push(parts);
    }
    if detail_profile {
        log_profile(
            "arrangement_finalize_group_rebuild",
            rebuild_groups_started_at,
        );
        log_profile("arrangement_finalize", finalize_started_at);
    }

    SegmentArrangement {
        segments,
        point_positions,
        group_segments,
    }
}

fn register_pair_splits(
    left: CanonicalSegment,
    right: CanonicalSegment,
    left_index: usize,
    right_index: usize,
    left_start: [f32; 2],
    left_end: [f32; 2],
    right_start: [f32; 2],
    right_end: [f32; 2],
    point_positions: &mut PointPositionIndex,
    split_points: &mut [Vec<QPoint>],
) {
    let o1 = orientation(left_start, left_end, right_start);
    let o2 = orientation(left_start, left_end, right_end);
    let colinear = o1.abs() <= AREA_EPSILON && o2.abs() <= AREA_EPSILON;
    let left_straddles_right =
        (o1 > AREA_EPSILON && o2 < -AREA_EPSILON) || (o1 < -AREA_EPSILON && o2 > AREA_EPSILON);

    if segments_share_endpoint(left, right) && !colinear {
        return;
    }

    if colinear {
        register_colinear_overlap_splits(
            left,
            right,
            left_index,
            right_index,
            left_start,
            left_end,
            right_start,
            right_end,
            point_positions,
            split_points,
        );
        return;
    }

    if !left_straddles_right && o1.abs() > AREA_EPSILON && o2.abs() > AREA_EPSILON {
        return;
    }

    let o3 = orientation(right_start, right_end, left_start);
    let o4 = orientation(right_start, right_end, left_end);

    let Some(point) = segment_intersection_point_from_orientations(
        left_start,
        left_end,
        right_start,
        right_end,
        o1,
        o2,
        o3,
        o4,
    ) else {
        return;
    };
    register_split_point_for_segment(&mut split_points[left_index], point_positions, left, point);
    register_split_point_for_segment(
        &mut split_points[right_index],
        point_positions,
        right,
        point,
    );
}

fn register_colinear_overlap_splits(
    left: CanonicalSegment,
    right: CanonicalSegment,
    left_index: usize,
    right_index: usize,
    left_start: [f32; 2],
    left_end: [f32; 2],
    right_start: [f32; 2],
    right_end: [f32; 2],
    point_positions: &mut PointPositionIndex,
    split_points: &mut [Vec<QPoint>],
) {
    for point in [left_start, left_end] {
        if point_on_segment(point, right_start, right_end) {
            register_split_point_for_segment(
                &mut split_points[right_index],
                point_positions,
                right,
                point,
            );
        }
    }
    for point in [right_start, right_end] {
        if point_on_segment(point, left_start, left_end) {
            register_split_point_for_segment(
                &mut split_points[left_index],
                point_positions,
                left,
                point,
            );
        }
    }
}

fn register_split_point_for_segment(
    split_points: &mut Vec<QPoint>,
    point_positions: &mut PointPositionIndex,
    segment: CanonicalSegment,
    point: [f32; 2],
) {
    let quantized = quantize_point(point);
    if is_segment_endpoint(segment, quantized) {
        return;
    }
    split_points.push(quantized);
    insert_point_position(point_positions, quantized, point);
}

fn log_split_point_profile(split_points: &[Vec<QPoint>]) {
    if !split_profile_enabled() {
        return;
    }

    let mut non_empty_segments = 0usize;
    let mut raw_total = 0usize;
    let mut unique_total = 0usize;
    let mut duplicate_total = 0usize;
    let mut max_raw_len = 0usize;

    for points in split_points {
        if points.is_empty() {
            continue;
        }
        non_empty_segments += 1;
        raw_total += points.len();
        max_raw_len = max_raw_len.max(points.len());

        let unique_len = if points.len() <= 1 {
            points.len()
        } else {
            let mut unique_points = points.clone();
            unique_points.sort_unstable();
            unique_points.dedup();
            unique_points.len()
        };
        unique_total += unique_len;
        duplicate_total += points.len() - unique_len;
    }

    eprintln!(
        "edge_drawer: split_stats segments={} raw_points={} unique_points={} duplicates={} max_points_per_segment={}",
        non_empty_segments,
        raw_total,
        unique_total,
        duplicate_total,
        max_raw_len,
    );
}

fn normalize_split_points(split_points: &mut [Vec<QPoint>]) {
    for points in split_points {
        if points.len() <= 1 {
            continue;
        }
        points.sort_unstable();
        points.dedup();
    }
}

fn is_segment_endpoint(segment: CanonicalSegment, point: QPoint) -> bool {
    point == segment.start || point == segment.end
}

fn dedup_sorted_segments_in_place(segments: &mut Vec<CanonicalSegment>) {
    if segments.len() <= 1 {
        return;
    }

    let mut write_index = 1usize;
    for read_index in 1..segments.len() {
        if segments[read_index] != segments[write_index - 1] {
            if write_index != read_index {
                segments[write_index] = segments[read_index];
            }
            write_index += 1;
        }
    }
    segments.truncate(write_index);
}

fn segments_share_endpoint(left: CanonicalSegment, right: CanonicalSegment) -> bool {
    left.start == right.start
        || left.start == right.end
        || left.end == right.start
        || left.end == right.end
}

fn split_segment(
    original: CanonicalSegment,
    split_points: Option<&[QPoint]>,
    _point_positions: &PointPositionIndex,
) -> Vec<CanonicalSegment> {
    let Some(split_points) = split_points else {
        return vec![original];
    };
    if split_points.is_empty() {
        return vec![original];
    }

    if split_points.len() == 1 {
        let point = split_points[0];
        let mut segments = Vec::with_capacity(2);
        if let Some(segment) = canonical_segment(original.start, point) {
            segments.push(segment);
        }
        if let Some(segment) = canonical_segment(point, original.end) {
            segments.push(segment);
        }
        return segments;
    }

    let mut segments = Vec::with_capacity(split_points.len() + 1);
    let mut previous = original.start;
    for &point in split_points {
        if let Some(segment) = canonical_segment(previous, point) {
            segments.push(segment);
        }
        previous = point;
    }
    if let Some(segment) = canonical_segment(previous, original.end) {
        segments.push(segment);
    }
    segments
}

#[cfg(test)]
fn classify_visible_segments(edges: &[Edges]) -> HashSet<CanonicalSegment> {
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

fn build_island_fills(
    polygons: &[Polygon],
    island_fill: Option<&IslandFillConfig>,
    width: u32,
    height: u32,
) -> Vec<PreparedFill> {
    let Some(island_fill) = island_fill else {
        return Vec::new();
    };
    if !island_fill.enabled {
        return Vec::new();
    }

    let opacity = island_fill.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 || polygons.is_empty() {
        return Vec::new();
    }
    let alpha = (opacity * 255.0).round() as u8;
    let padding_pixels = island_fill.padding_pixels.max(0.0);

    build_island_fills_from_polygons(polygons, alpha, padding_pixels, width, height)
}

fn build_island_fills_from_polygons(
    polygons: &[Polygon],
    alpha: u8,
    padding_pixels: f32,
    width: u32,
    height: u32,
) -> Vec<PreparedFill> {
    let overlay_shapes = polygons_to_overlay_shapes(polygons, width, height);
    if overlay_shapes.is_empty() {
        return Vec::new();
    }

    let mut overlay = FloatOverlay::with_subj(&overlay_shapes);
    let mut union_shapes = overlay.overlay(OverlayRule::Subject, OverlayFillRule::NonZero);
    union_shapes.sort_by(|left, right| {
        let left_bounds = overlay_shape_bounds(left);
        let right_bounds = overlay_shape_bounds(right);
        compare_overlay_shape_bounds(left_bounds, right_bounds)
    });

    let mut fills = Vec::with_capacity(union_shapes.len());
    for shape in union_shapes {
        let Some(fill_shape) = overlay_shape_to_fill_shape(&shape, width, height) else {
            continue;
        };
        if fill_shape.is_empty() {
            continue;
        }
        let fill_index = fills.len();
        fills.push(PreparedFill {
            fill_color: island_fill_color(fill_index, alpha),
            padding_pixels,
            shapes: vec![fill_shape],
        });
    }

    fills
}

fn polygons_to_overlay_shapes(polygons: &[Polygon], width: u32, height: u32) -> OverlayShapes {
    polygons
        .iter()
        .filter_map(|polygon| polygon_to_overlay_shape(polygon, width, height))
        .collect()
}

fn polygon_to_overlay_shape(polygon: &Polygon, width: u32, height: u32) -> Option<OverlayShape> {
    let contour = polygon_to_overlay_contour(polygon, width, height)?;
    Some(vec![contour])
}

fn polygon_to_overlay_contour(
    polygon: &Polygon,
    width: u32,
    height: u32,
) -> Option<OverlayContour> {
    if polygon.points.len() < 3 || width == 0 || height == 0 {
        return None;
    }

    let mut contour = Vec::with_capacity(polygon.points.len());
    for &point in &polygon.points {
        let canvas = uv_to_canvas_point(point, width, height);
        if contour.last().copied() == Some(canvas) {
            continue;
        }
        contour.push(canvas);
    }
    if contour.len() >= 3 && contour.first() == contour.last() {
        contour.pop();
    }
    if contour.len() < 3 {
        return None;
    }
    if overlay_contour_area(&contour) < 0.0 {
        contour.reverse();
    }
    Some(contour)
}

fn overlay_shapes_bounds(shapes: &OverlayShapes) -> Option<OverlayBounds> {
    let mut bounds: Option<OverlayBounds> = None;
    for shape in shapes {
        for contour in shape {
            for point in contour {
                bounds = Some(match bounds {
                    Some(current) => OverlayBounds {
                        min_x: current.min_x.min(point[0]),
                        min_y: current.min_y.min(point[1]),
                        max_x: current.max_x.max(point[0]),
                        max_y: current.max_y.max(point[1]),
                    },
                    None => OverlayBounds {
                        min_x: point[0],
                        min_y: point[1],
                        max_x: point[0],
                        max_y: point[1],
                    },
                });
            }
        }
    }
    bounds
}

fn overlay_shape_bounds(shape: &OverlayShape) -> Option<OverlayBounds> {
    let mut bounds: Option<OverlayBounds> = None;
    for contour in shape {
        for point in contour {
            bounds = Some(match bounds {
                Some(current) => OverlayBounds {
                    min_x: current.min_x.min(point[0]),
                    min_y: current.min_y.min(point[1]),
                    max_x: current.max_x.max(point[0]),
                    max_y: current.max_y.max(point[1]),
                },
                None => OverlayBounds {
                    min_x: point[0],
                    min_y: point[1],
                    max_x: point[0],
                    max_y: point[1],
                },
            });
        }
    }
    bounds
}

fn compare_overlay_shape_bounds(
    left: Option<OverlayBounds>,
    right: Option<OverlayBounds>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .min_y
            .total_cmp(&right.min_y)
            .then_with(|| left.min_x.total_cmp(&right.min_x))
            .then_with(|| left.max_y.total_cmp(&right.max_y))
            .then_with(|| left.max_x.total_cmp(&right.max_x)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn fill_shapes_to_overlay_shapes(shapes: &[FillShape], width: u32, height: u32) -> OverlayShapes {
    shapes
        .iter()
        .filter_map(|shape| fill_shape_to_overlay_shape(shape, width, height))
        .collect()
}

fn fill_shape_to_overlay_shape(shape: &FillShape, width: u32, height: u32) -> Option<OverlayShape> {
    let contours = shape
        .iter()
        .filter_map(|contour| fill_contour_to_overlay_contour(contour, width, height))
        .collect::<Vec<_>>();
    if contours.is_empty() {
        None
    } else {
        Some(contours)
    }
}

fn fill_contour_to_overlay_contour(
    contour: &[QPoint],
    width: u32,
    height: u32,
) -> Option<OverlayContour> {
    if contour.len() < 4 || contour.first() != contour.last() {
        return None;
    }

    let mut overlay = contour[..contour.len() - 1]
        .iter()
        .map(|point| {
            let canvas = to_canvas_point(*point, width, height);
            [canvas[0] as f64, canvas[1] as f64]
        })
        .collect::<Vec<_>>();
    overlay.dedup_by(|left, right| {
        (left[0] - right[0]).abs() < f64::EPSILON && (left[1] - right[1]).abs() < f64::EPSILON
    });
    if overlay.len() < 3 {
        return None;
    }
    if overlay_contour_area(&overlay) < 0.0 {
        overlay.reverse();
    }
    Some(overlay)
}

fn overlay_shape_to_fill_shape(shape: &OverlayShape, width: u32, height: u32) -> Option<FillShape> {
    let contours = shape
        .iter()
        .filter_map(|contour| overlay_contour_to_fill_contour(contour, width, height))
        .collect::<Vec<_>>();
    if contours.is_empty() {
        None
    } else {
        Some(contours)
    }
}

fn overlay_contour_to_fill_contour(
    contour: &OverlayContour,
    width: u32,
    height: u32,
) -> Option<FillContour> {
    let mut fill_contour = contour
        .iter()
        .map(|point| QPoint {
            x: ((point[0] / width as f64) * QUANTIZE_SCALE as f64).round() as i64,
            y: (((height as f64 - point[1]) / height as f64) * QUANTIZE_SCALE as f64).round()
                as i64,
        })
        .collect::<Vec<_>>();
    fill_contour.dedup();
    if fill_contour.len() < 3 {
        return None;
    }
    if fill_contour.first() != fill_contour.last() {
        fill_contour.push(fill_contour[0]);
    }
    Some(fill_contour)
}

fn overlay_contour_area(contour: &OverlayContour) -> f64 {
    let mut area = 0.0;
    for idx in 0..contour.len() {
        let current = contour[idx];
        let next = contour[(idx + 1) % contour.len()];
        area += current[0] * next[1] - next[0] * current[1];
    }
    area * 0.5
}

fn island_fill_color(index: usize, alpha: u8) -> [u8; 4] {
    let rgb = ISLAND_FILL_PALETTE[index % ISLAND_FILL_PALETTE.len()];
    [rgb[0], rgb[1], rgb[2], alpha]
}

fn collect_arrangement_inputs(edges: &[Edges]) -> ArrangementInputs {
    let detail_profile = arrangement_detail_profile_enabled();
    let line_count = edges.iter().map(|group| group.lines.len()).sum::<usize>();
    let collect_segments_started_at = Instant::now();
    let mut point_positions = HashMap::with_capacity(line_count.saturating_mul(2));
    let mut original_segments = Vec::with_capacity(line_count);
    let mut group_segments = Vec::with_capacity(edges.len());

    for group in edges {
        let mut parts = Vec::with_capacity(group.lines.len());
        for line in &group.lines {
            let start = quantize_point(line.uv1);
            let end = quantize_point(line.uv2);
            point_positions.entry(start).or_insert(line.uv1);
            point_positions.entry(end).or_insert(line.uv2);

            let Some(segment) = canonical_segment(start, end) else {
                continue;
            };
            original_segments.push(segment);
            parts.push(segment);
        }

        parts.sort_unstable_by_key(|segment| (segment.start, segment.end));
        parts.dedup();
        group_segments.push(parts);
    }
    if detail_profile {
        log_profile("arrangement_collect_segments", collect_segments_started_at);
    }

    let sort_original_started_at = Instant::now();
    original_segments.sort_unstable_by_key(|segment| (segment.start, segment.end));
    original_segments.dedup();
    if detail_profile {
        log_profile(
            "arrangement_collect_original_dedup",
            sort_original_started_at,
        );
    }

    let materialize_group_indices_started_at = Instant::now();
    let group_segment_indices = build_group_segment_indices(&original_segments, &group_segments);
    if detail_profile {
        log_profile(
            "arrangement_collect_group_indices",
            materialize_group_indices_started_at,
        );
    }

    ArrangementInputs {
        point_positions: Arc::new(point_positions),
        original_segments,
        group_segments,
        group_segment_indices,
    }
}

fn build_group_segment_indices(
    original_segments: &[CanonicalSegment],
    group_segments: &[Vec<CanonicalSegment>],
) -> Vec<Vec<usize>> {
    group_segments
        .iter()
        .map(|group| {
            let mut cursor = 0usize;
            let mut indices = Vec::with_capacity(group.len());

            for &segment in group {
                let target = (segment.start, segment.end);
                while cursor < original_segments.len()
                    && (
                        original_segments[cursor].start,
                        original_segments[cursor].end,
                    ) < target
                {
                    cursor += 1;
                }
                assert!(
                    cursor < original_segments.len() && original_segments[cursor] == segment,
                    "group segment must exist in original segments",
                );
                indices.push(cursor);
            }

            indices
        })
        .collect()
}

fn default_grid_resolution(count: usize) -> u32 {
    if count <= 1 {
        return 16;
    }

    let target = (count as f32).sqrt().ceil() as u32;
    target.next_power_of_two().clamp(16, 256)
}

fn default_candidate_pair_grid_resolution(count: usize) -> u32 {
    if count <= 1 {
        return 32;
    }

    let target = ((count as f32).sqrt().ceil() as u32).saturating_mul(2);
    target.next_power_of_two().clamp(32, 256)
}

fn candidate_pair_grid_resolution(count: usize) -> u32 {
    if let Ok(value) = std::env::var("EDGE_DRAWER_PAIR_GRID_RESOLUTION") {
        if let Ok(parsed) = value.parse::<u32>() {
            if parsed >= 1 {
                return parsed.next_power_of_two().clamp(32, 1024);
            }
        }
    }

    default_candidate_pair_grid_resolution(count)
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

fn build_dense_grid(bounds: &[SegmentBounds], expand: f32, resolution: u32) -> DenseGridIndex {
    let (min, max) = combine_bounds(bounds, expand);
    let cell_size = [
        (max[0] - min[0]) / resolution as f32,
        (max[1] - min[1]) / resolution as f32,
    ];
    let cell_count = (resolution as usize) * (resolution as usize);
    let mut counts = vec![0usize; cell_count];
    for bounds in bounds.iter() {
        let min_x = cell_coord(bounds.min[0] - expand, min[0], cell_size[0], resolution);
        let max_x = cell_coord(bounds.max[0] + expand, min[0], cell_size[0], resolution);
        let min_y = cell_coord(bounds.min[1] - expand, min[1], cell_size[1], resolution);
        let max_y = cell_coord(bounds.max[1] + expand, min[1], cell_size[1], resolution);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                counts[dense_cell_index(x, y, resolution)] += 1;
            }
        }
    }

    let mut cell_starts = vec![0usize; cell_count + 1];
    for index in 0..cell_count {
        cell_starts[index + 1] = cell_starts[index] + counts[index];
    }
    let mut indices = vec![0usize; cell_starts[cell_count]];
    let mut write_offsets = cell_starts[..cell_count].to_vec();

    for (idx, bounds) in bounds.iter().enumerate() {
        let min_x = cell_coord(bounds.min[0] - expand, min[0], cell_size[0], resolution);
        let max_x = cell_coord(bounds.max[0] + expand, min[0], cell_size[0], resolution);
        let min_y = cell_coord(bounds.min[1] - expand, min[1], cell_size[1], resolution);
        let max_y = cell_coord(bounds.max[1] + expand, min[1], cell_size[1], resolution);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                let cell_index = dense_cell_index(x, y, resolution);
                let write_index = write_offsets[cell_index];
                indices[write_index] = idx;
                write_offsets[cell_index] += 1;
            }
        }
    }

    DenseGridIndex {
        min,
        cell_size,
        resolution,
        cell_starts,
        indices,
    }
}

fn build_arrangement_grid(
    bounds: &[SegmentBounds],
    expand: f32,
    resolution: u32,
) -> ArrangementGridIndex {
    let dense = build_dense_grid(bounds, expand, resolution);
    let mut segment_cell_starts = Vec::with_capacity(bounds.len() + 1);
    let mut segment_cells = Vec::new();
    segment_cell_starts.push(0);

    for bounds in bounds.iter() {
        let min_x = cell_coord(
            bounds.min[0] - expand,
            dense.min[0],
            dense.cell_size[0],
            dense.resolution,
        );
        let max_x = cell_coord(
            bounds.max[0] + expand,
            dense.min[0],
            dense.cell_size[0],
            dense.resolution,
        );
        let min_y = cell_coord(
            bounds.min[1] - expand,
            dense.min[1],
            dense.cell_size[1],
            dense.resolution,
        );
        let max_y = cell_coord(
            bounds.max[1] + expand,
            dense.min[1],
            dense.cell_size[1],
            dense.resolution,
        );

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                segment_cells.push(dense_cell_index(x, y, dense.resolution));
            }
        }

        segment_cell_starts.push(segment_cells.len());
    }

    ArrangementGridIndex {
        dense,
        segment_cell_starts,
        segment_cells,
    }
}

fn visit_candidate_pairs<F>(bounds: &[SegmentBounds], expand: f32, mut visitor: F)
where
    F: FnMut(usize, usize),
{
    if bounds.len() < 2 {
        return;
    }

    let resolution = candidate_pair_grid_resolution(bounds.len());
    let grid = build_arrangement_grid(bounds, expand, resolution);
    let mut visited_marks = vec![0u32; bounds.len()];
    let mut current_mark = 1u32;
    let pair_profile = pair_profile_enabled();
    let mut segment_cell_visits = 0usize;
    let mut dense_candidates = 0usize;
    let mut duplicate_candidates = 0usize;
    let mut ordered_pair_candidates = 0usize;
    let mut overlap_candidates = 0usize;

    for current in 0..bounds.len() {
        if current_mark == u32::MAX {
            visited_marks.fill(0);
            current_mark = 1;
        }

        let current_bounds = bounds[current];
        let cell_start = grid.segment_cell_starts[current];
        let cell_end = grid.segment_cell_starts[current + 1];
        if pair_profile {
            segment_cell_visits += cell_end - cell_start;
        }

        for &cell_index in &grid.segment_cells[cell_start..cell_end] {
            let start = grid.dense.cell_starts[cell_index];
            let end = grid.dense.cell_starts[cell_index + 1];
            let candidates = &grid.dense.indices[start..end];
            let first_higher = candidates.partition_point(|&candidate| candidate <= current);

            for &candidate in &candidates[first_higher..] {
                if pair_profile {
                    dense_candidates += 1;
                }
                if pair_profile {
                    ordered_pair_candidates += 1;
                }
                if visited_marks[candidate] == current_mark {
                    if pair_profile {
                        duplicate_candidates += 1;
                    }
                    continue;
                }
                visited_marks[candidate] = current_mark;

                if bounds_overlap(current_bounds, bounds[candidate], expand) {
                    if pair_profile {
                        overlap_candidates += 1;
                    }
                    visitor(current, candidate);
                }
            }
        }

        current_mark += 1;
    }

    if pair_profile {
        eprintln!(
            "edge_drawer: pair_stats resolution={} cells={} dense_candidates={} ordered_candidates={} duplicates={} overlaps={}",
            resolution,
            segment_cell_visits,
            dense_candidates,
            ordered_pair_candidates,
            duplicate_candidates,
            overlap_candidates,
        );
    }
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

fn dense_cell_index(x: i32, y: i32, resolution: u32) -> usize {
    (y as usize) * (resolution as usize) + (x as usize)
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
    let cell_size = [width_f / resolution as f32, height_f / resolution as f32];
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for (idx, segment) in segments.iter().enumerate() {
        let bounds = segment_bounds_canvas(*segment, width, height);
        let min_x = cell_coord(
            (bounds.min[0] - expand_pixels).max(0.0),
            0.0,
            cell_size[0],
            resolution,
        );
        let max_x = cell_coord(
            (bounds.max[0] + expand_pixels).min(width_f),
            0.0,
            cell_size[0],
            resolution,
        );
        let min_y = cell_coord(
            (bounds.min[1] - expand_pixels).max(0.0),
            0.0,
            cell_size[1],
            resolution,
        );
        let max_y = cell_coord(
            (bounds.max[1] + expand_pixels).min(height_f),
            0.0,
            cell_size[1],
            resolution,
        );

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                cells.entry((x, y)).or_default().push(idx);
            }
        }
    }

    UniformGridIndex { cells }
}

fn detect_padding_warning_segments(
    outline_segments: &HashSet<CanonicalSegment>,
    point_positions: &PointPositionIndex,
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
        .map(|segment| {
            outline_components
                .get(&segment.start)
                .copied()
                .unwrap_or_default()
        })
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

fn segment_border_distance_pixels(segment: CanonicalSegment, width: u32, height: u32) -> f32 {
    let a = to_canvas_point(segment.start, width, height);
    let b = to_canvas_point(segment.end, width, height);
    let min_x = a[0].min(b[0]);
    let max_x = a[0].max(b[0]);
    let min_y = a[1].min(b[1]);
    let max_y = a[1].max(b[1]);
    let width = width as f32;
    let height = height as f32;

    min_x
        .min((width - max_x).max(0.0))
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

    segments_intersect_from_orientations(a0, a1, b0, b1, o1, o2, o3, o4)
}

fn segments_intersect_from_orientations(
    a0: [f32; 2],
    a1: [f32; 2],
    b0: [f32; 2],
    b1: [f32; 2],
    o1: f32,
    o2: f32,
    o3: f32,
    o4: f32,
) -> bool {
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

fn point_on_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> bool {
    orientation(start, end, point).abs() <= AREA_EPSILON && on_segment(start, point, end)
}

fn segment_intersection_point_from_orientations(
    a0: [f32; 2],
    a1: [f32; 2],
    b0: [f32; 2],
    b1: [f32; 2],
    o1: f32,
    o2: f32,
    o3: f32,
    o4: f32,
) -> Option<[f32; 2]> {
    if !segments_intersect_from_orientations(a0, a1, b0, b1, o1, o2, o3, o4) {
        return None;
    }

    let denominator = (a0[0] - a1[0]) * (b0[1] - b1[1]) - (a0[1] - a1[1]) * (b0[0] - b1[0]);
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
    point_positions: &PointPositionIndex,
    polygons: &[Polygon],
) -> (HashSet<CanonicalSegment>, HashSet<CanonicalSegment>) {
    if polygons.is_empty() {
        return classify_segments_from_graph(unique_segments, point_positions);
    }

    let polygon_index = build_polygon_index(polygons);
    let mut stats = classification_detail_profile_enabled().then(ClassificationStats::default);
    let mut internal_segments = HashSet::with_capacity(unique_segments.len());
    let mut outline_segments = HashSet::with_capacity(unique_segments.len());

    for &segment in unique_segments {
        let (left_inside, right_inside) = segment_side_states_with_polygons(
            segment,
            &polygon_index,
            point_positions,
            stats.as_mut(),
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

    if let Some(stats) = stats {
        eprintln!(
            "edge_drawer: classification_stats sample_queries={} candidate_polygons={} bounds_checks={} point_tests={}",
            stats.sample_queries,
            stats.candidate_polygons,
            stats.bounds_checks,
            stats.point_in_polygon_tests,
        );
    }

    (internal_segments, outline_segments)
}

fn classify_segments_from_graph(
    unique_segments: &[CanonicalSegment],
    point_positions: &PointPositionIndex,
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
        let component_id = component_map
            .get(&segment.start)
            .copied()
            .unwrap_or_default();
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
    point_positions: &PointPositionIndex,
    stats: Option<&mut ClassificationStats>,
) -> (bool, bool) {
    let a = point_position(point_positions, segment.start);
    let b = point_position(point_positions, segment.end);
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

    if let Some(stats) = stats {
        segment_side_states_with_shared_candidates(left, right, polygon_index, stats)
    } else {
        segment_side_states_with_shared_candidates_no_stats(left, right, polygon_index)
    }
}

fn segment_side_states_with_shared_candidates(
    left: [f32; 2],
    right: [f32; 2],
    polygon_index: &PolygonIndex<'_>,
    stats: &mut ClassificationStats,
) -> (bool, bool) {
    stats.sample_queries += 2;

    let left_cell = dense_grid_cell_index_for_point(&polygon_index.grid, left);
    let right_cell = dense_grid_cell_index_for_point(&polygon_index.grid, right);
    if left_cell != right_cell {
        let left_inside = point_in_polygons_from_candidates(
            left,
            polygon_index,
            dense_grid_candidates_for_cell(&polygon_index.grid, left_cell),
            Some(&mut *stats),
        );
        let right_inside = point_in_polygons_from_candidates(
            right,
            polygon_index,
            dense_grid_candidates_for_cell(&polygon_index.grid, right_cell),
            Some(stats),
        );
        return (left_inside, right_inside);
    }

    let candidates = dense_grid_candidates_for_cell(&polygon_index.grid, left_cell);
    stats.candidate_polygons += candidates.len() * 2;

    let mut left_inside = false;
    let mut right_inside = false;
    for &polygon_index_id in candidates {
        if left_inside && right_inside {
            break;
        }

        let polygon = &polygon_index.polygons[polygon_index_id];
        if !left_inside {
            stats.bounds_checks += 1;
            if bounds_contains_point(polygon.bounds, left) {
                stats.point_in_polygon_tests += 1;
                if point_in_polygon_points(left, &polygon.points) {
                    left_inside = true;
                }
            }
        }
        if !right_inside {
            stats.bounds_checks += 1;
            if bounds_contains_point(polygon.bounds, right) {
                stats.point_in_polygon_tests += 1;
                if point_in_polygon_points(right, &polygon.points) {
                    right_inside = true;
                }
            }
        }
    }

    (left_inside, right_inside)
}

fn segment_side_states_with_shared_candidates_no_stats(
    left: [f32; 2],
    right: [f32; 2],
    polygon_index: &PolygonIndex<'_>,
) -> (bool, bool) {
    let left_cell = dense_grid_cell_index_for_point(&polygon_index.grid, left);
    let right_cell = dense_grid_cell_index_for_point(&polygon_index.grid, right);
    if left_cell != right_cell {
        let left_inside = point_in_polygons_from_candidates(
            left,
            polygon_index,
            dense_grid_candidates_for_cell(&polygon_index.grid, left_cell),
            None,
        );
        let right_inside = point_in_polygons_from_candidates(
            right,
            polygon_index,
            dense_grid_candidates_for_cell(&polygon_index.grid, right_cell),
            None,
        );
        return (left_inside, right_inside);
    }

    let candidates = dense_grid_candidates_for_cell(&polygon_index.grid, left_cell);
    let mut left_inside = false;
    let mut right_inside = false;
    for &polygon_index_id in candidates {
        if left_inside && right_inside {
            break;
        }

        let polygon = &polygon_index.polygons[polygon_index_id];
        if !left_inside
            && bounds_contains_point(polygon.bounds, left)
            && point_in_polygon_points(left, &polygon.points)
        {
            left_inside = true;
        }
        if !right_inside
            && bounds_contains_point(polygon.bounds, right)
            && point_in_polygon_points(right, &polygon.points)
        {
            right_inside = true;
        }
    }

    (left_inside, right_inside)
}

fn segment_side_states(
    segment: CanonicalSegment,
    faces: &[FaceLoop],
    point_positions: &PointPositionIndex,
) -> (bool, bool) {
    let a = point_position(point_positions, segment.start);
    let b = point_position(point_positions, segment.end);
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
    point_positions: &PointPositionIndex,
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
        let origin = point_position(point_positions, *point);
        neighbors.sort_by(|lhs, rhs| {
            let left = point_position(point_positions, *lhs);
            let right = point_position(point_positions, *rhs);
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
    point_positions: &PointPositionIndex,
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
    point_positions: &PointPositionIndex,
) -> bool {
    let mut inside = false;
    for face in faces {
        if point_in_polygon(point, &face.points, point_positions) {
            inside = !inside;
        }
    }
    inside
}

fn build_polygon_index(polygons: &[Polygon]) -> PolygonIndex<'_> {
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
                points: polygon.points.as_slice(),
                bounds: SegmentBounds { min, max },
            }
        })
        .collect::<Vec<_>>();
    let bounds = indexed_polygons
        .iter()
        .map(|polygon| polygon.bounds)
        .collect::<Vec<_>>();
    let grid = build_dense_grid(&bounds, 0.0, default_grid_resolution(bounds.len()));
    PolygonIndex {
        polygons: indexed_polygons,
        grid,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn point_in_polygons(
    point: [f32; 2],
    polygon_index: &PolygonIndex<'_>,
    mut stats: Option<&mut ClassificationStats>,
) -> bool {
    if let Some(stats) = stats.as_mut() {
        stats.sample_queries += 1;
    }

    let candidates = dense_grid_candidates_for_point(&polygon_index.grid, point);
    point_in_polygons_from_candidates(point, polygon_index, candidates, stats)
}

fn point_in_polygons_from_candidates(
    point: [f32; 2],
    polygon_index: &PolygonIndex<'_>,
    candidates: &[usize],
    mut stats: Option<&mut ClassificationStats>,
) -> bool {
    if let Some(stats) = stats.as_mut() {
        stats.candidate_polygons += candidates.len();
    }
    for &polygon_index_id in candidates {
        let polygon = &polygon_index.polygons[polygon_index_id];
        if let Some(stats) = stats.as_mut() {
            stats.bounds_checks += 1;
        }
        if !bounds_contains_point(polygon.bounds, point) {
            continue;
        }
        if let Some(stats) = stats.as_mut() {
            stats.point_in_polygon_tests += 1;
        }
        if point_in_polygon_points(point, &polygon.points) {
            return true;
        }
    }
    false
}

#[cfg_attr(not(test), allow(dead_code))]
fn dense_grid_candidates_for_point(grid: &DenseGridIndex, point: [f32; 2]) -> &[usize] {
    let cell_index = dense_grid_cell_index_for_point(grid, point);
    dense_grid_candidates_for_cell(grid, cell_index)
}

fn dense_grid_cell_index_for_point(grid: &DenseGridIndex, point: [f32; 2]) -> usize {
    let x = cell_coord(point[0], grid.min[0], grid.cell_size[0], grid.resolution);
    let y = cell_coord(point[1], grid.min[1], grid.cell_size[1], grid.resolution);
    dense_cell_index(x, y, grid.resolution)
}

fn dense_grid_candidates_for_cell(grid: &DenseGridIndex, cell_index: usize) -> &[usize] {
    let start = grid.cell_starts[cell_index];
    let end = grid.cell_starts[cell_index + 1];
    &grid.indices[start..end]
}

fn bounds_contains_point(bounds: SegmentBounds, point: [f32; 2]) -> bool {
    point[0] >= bounds.min[0] - AREA_EPSILON
        && point[0] <= bounds.max[0] + AREA_EPSILON
        && point[1] >= bounds.min[1] - AREA_EPSILON
        && point[1] <= bounds.max[1] + AREA_EPSILON
}

fn point_in_polygon_points(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    if polygon.len() == 3 {
        return point_in_triangle(point, polygon[0], polygon[1], polygon[2]);
    }

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

fn point_in_triangle(point: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let ab = orientation(a, b, point);
    let bc = orientation(b, c, point);
    let ca = orientation(c, a, point);
    let has_negative = ab < -AREA_EPSILON || bc < -AREA_EPSILON || ca < -AREA_EPSILON;
    let has_positive = ab > AREA_EPSILON || bc > AREA_EPSILON || ca > AREA_EPSILON;
    !(has_negative && has_positive)
}

fn point_in_polygon(
    point: [f32; 2],
    polygon: &[QPoint],
    point_positions: &PointPositionIndex,
) -> bool {
    let mut inside = false;
    let mut previous = point_position(
        point_positions,
        *polygon.last().expect("polygon is non-empty"),
    );
    for vertex in polygon {
        let current = point_position(point_positions, *vertex);
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

fn polygon_area(points: &[QPoint], point_positions: &PointPositionIndex) -> f32 {
    let mut area = 0.0;
    for idx in 0..points.len() {
        let current = point_position(point_positions, points[idx]);
        let next = point_position(point_positions, points[(idx + 1) % points.len()]);
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

    for fill in &prepared.fills {
        if fill.shapes.is_empty() {
            continue;
        }

        let color = format!(
            "rgb({}, {}, {})",
            fill.fill_color[0], fill.fill_color[1], fill.fill_color[2]
        );
        let opacity = fill.fill_color[3] as f32 / 255.0;

        for shape in &fill.shapes {
            let Some(data) = svg_shape_data(shape, width, height) else {
                continue;
            };

            let svg_path = SvgPath::new()
                .set("fill", color.clone())
                .set("fill-opacity", opacity)
                .set("stroke", color.clone())
                .set("stroke-opacity", opacity)
                .set("stroke-width", fill.padding_pixels * 2.0)
                .set("stroke-linecap", "round")
                .set("stroke-linejoin", "round")
                .set("d", data);

            document = document.add(svg_path);
        }
    }

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

fn svg_shape_data(shape: &FillShape, width: u32, height: u32) -> Option<Data> {
    let mut data = Data::new();
    let mut has_contour = false;

    for contour in shape {
        let Some(first) = contour.first() else {
            continue;
        };
        let first_point = to_canvas_point(*first, width, height);
        data = data.move_to((first_point[0] as f64, first_point[1] as f64));

        for point in &contour[1..] {
            let canvas = to_canvas_point(*point, width, height);
            data = data.line_to((canvas[0] as f64, canvas[1] as f64));
        }

        if contour.len() >= 3 && contour.first() == contour.last() {
            data = data.close();
        }
        has_contour = true;
    }

    if has_contour {
        Some(data)
    } else {
        None
    }
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

    let use_distance_field_fill = prepared.fills.iter().any(|fill| fill.padding_pixels > 0.0);
    if use_distance_field_fill {
        draw_island_fill_distance_field_raster(&mut pixmap, &prepared.fills, width, height);
    }

    for fill in &prepared.fills {
        if use_distance_field_fill && fill.padding_pixels > 0.0 {
            continue;
        }
        if fill.shapes.is_empty() {
            continue;
        }

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(
            fill.fill_color[0],
            fill.fill_color[1],
            fill.fill_color[2],
            fill.fill_color[3],
        ));

        for shape in &fill.shapes {
            let Some(sk_path) = build_skia_shape(shape, width, height) else {
                continue;
            };
            pixmap.fill_path(
                &sk_path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

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

fn draw_island_fill_distance_field_raster(
    pixmap: &mut Pixmap,
    fills: &[PreparedFill],
    width: u32,
    height: u32,
) {
    if fills.is_empty() || width == 0 || height == 0 {
        return;
    }
    if !fills.iter().any(|fill| fill.padding_pixels > 0.0) {
        return;
    }

    let fill_shapes = fills
        .iter()
        .map(|fill| fill_shapes_to_overlay_shapes(&fill.shapes, width, height))
        .collect::<Vec<_>>();
    let fill_bounds = fill_shapes
        .iter()
        .map(|shapes| overlay_shapes_bounds(shapes))
        .collect::<Vec<_>>();
    let pixel_count = (width as usize).saturating_mul(height as usize);
    let mut body_mask = vec![false; pixel_count];
    let mut owner_distances = vec![f64::INFINITY; pixel_count];
    let mut owner_indices = vec![usize::MAX; pixel_count];

    for (fill_index, shapes) in fill_shapes.iter().enumerate() {
        let Some(bounds) = fill_bounds[fill_index] else {
            continue;
        };
        let (min_x, max_x, min_y, max_y) = pixel_range_for_bounds(bounds, 0.0, width, height);
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let point = pixel_center(x, y);
                if point_in_overlay_shapes(point, shapes) {
                    let index = pixel_index(x, y, width);
                    body_mask[index] = true;
                    owner_distances[index] = -1.0;
                    owner_indices[index] = fill_index;
                }
            }
        }
    }

    for (fill_index, fill) in fills.iter().enumerate() {
        let padding_pixels = fill.padding_pixels.max(0.0) as f64;
        if padding_pixels <= 0.0 {
            continue;
        }
        let Some(bounds) = fill_bounds[fill_index] else {
            continue;
        };
        let padding_squared = padding_pixels * padding_pixels;
        let (min_x, max_x, min_y, max_y) =
            pixel_range_for_bounds(bounds, padding_pixels, width, height);
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let index = pixel_index(x, y, width);
                if body_mask[index] {
                    continue;
                }
                let point = pixel_center(x, y);
                let distance = distance_to_overlay_shapes_squared(point, &fill_shapes[fill_index]);
                if distance <= padding_squared && distance < owner_distances[index] {
                    owner_distances[index] = distance;
                    owner_indices[index] = fill_index;
                }
            }
        }
    }

    let pixels = pixmap.pixels_mut();
    for (index, owner_index) in owner_indices.into_iter().enumerate() {
        if owner_index == usize::MAX {
            continue;
        }
        source_over_pixel(&mut pixels[index], fills[owner_index].fill_color);
    }
}

fn pixel_range_for_bounds(
    bounds: OverlayBounds,
    expand: f64,
    width: u32,
    height: u32,
) -> (u32, u32, u32, u32) {
    let max_x_limit = width.saturating_sub(1) as f64;
    let max_y_limit = height.saturating_sub(1) as f64;
    let min_x = (bounds.min_x - expand).floor().clamp(0.0, max_x_limit) as u32;
    let max_x = (bounds.max_x + expand).ceil().clamp(0.0, max_x_limit) as u32;
    let min_y = (bounds.min_y - expand).floor().clamp(0.0, max_y_limit) as u32;
    let max_y = (bounds.max_y + expand).ceil().clamp(0.0, max_y_limit) as u32;
    (min_x, max_x, min_y, max_y)
}

fn pixel_center(x: u32, y: u32) -> OverlayPoint {
    [x as f64 + 0.5, y as f64 + 0.5]
}

fn pixel_index(x: u32, y: u32, width: u32) -> usize {
    y as usize * width as usize + x as usize
}

fn point_in_overlay_shapes(point: OverlayPoint, shapes: &OverlayShapes) -> bool {
    shapes.iter().any(|shape| {
        shape.iter().fold(false, |inside, contour| {
            inside ^ point_in_overlay_contour(point, contour)
        })
    })
}

fn point_in_overlay_contour(point: OverlayPoint, contour: &OverlayContour) -> bool {
    if contour.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = *contour.last().expect("contour is non-empty");
    for &current in contour {
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

fn distance_to_overlay_shapes_squared(point: OverlayPoint, shapes: &OverlayShapes) -> f64 {
    let mut best = f64::INFINITY;
    for shape in shapes {
        for contour in shape {
            best = best.min(distance_to_overlay_contour_squared(point, contour));
        }
    }
    best
}

fn distance_to_overlay_contour_squared(point: OverlayPoint, contour: &OverlayContour) -> f64 {
    if contour.is_empty() {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for index in 0..contour.len() {
        let start = contour[index];
        let end = contour[(index + 1) % contour.len()];
        best = best.min(distance_to_segment_squared(point, start, end));
    }
    best
}

fn distance_to_segment_squared(point: OverlayPoint, start: OverlayPoint, end: OverlayPoint) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f64::EPSILON {
        return squared_distance(point, start);
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / length_squared)
        .clamp(0.0, 1.0);
    squared_distance(point, [start[0] + dx * t, start[1] + dy * t])
}

fn squared_distance(left: OverlayPoint, right: OverlayPoint) -> f64 {
    let dx = left[0] - right[0];
    let dy = left[1] - right[1];
    dx * dx + dy * dy
}

fn source_over_pixel(pixel: &mut PremultipliedColorU8, color: [u8; 4]) {
    let src_a = color[3] as u16;
    if src_a == 0 {
        return;
    }
    let inv_src_a = 255u16.saturating_sub(src_a);
    let src_r = premultiply_channel(color[0], color[3]);
    let src_g = premultiply_channel(color[1], color[3]);
    let src_b = premultiply_channel(color[2], color[3]);
    let out_r = src_r as u16 + (pixel.red() as u16 * inv_src_a + 127) / 255;
    let out_g = src_g as u16 + (pixel.green() as u16 * inv_src_a + 127) / 255;
    let out_b = src_b as u16 + (pixel.blue() as u16 * inv_src_a + 127) / 255;
    let out_a = src_a + (pixel.alpha() as u16 * inv_src_a + 127) / 255;
    *pixel = PremultipliedColorU8::from_rgba(
        out_r.min(255) as u8,
        out_g.min(255) as u8,
        out_b.min(255) as u8,
        out_a.min(255) as u8,
    )
    .expect("source-over premultiplied color must be valid");
}

fn premultiply_channel(channel: u8, alpha: u8) -> u8 {
    ((channel as u16 * alpha as u16 + 127) / 255) as u8
}

fn build_skia_shape(shape: &FillShape, width: u32, height: u32) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    let mut has_contour = false;

    for contour in shape {
        let Some(first) = contour.first().copied() else {
            continue;
        };
        let first_point = to_canvas_point(first, width, height);
        builder.move_to(first_point[0], first_point[1]);

        for point in &contour[1..] {
            let canvas = to_canvas_point(*point, width, height);
            builder.line_to(canvas[0], canvas[1]);
        }

        if contour.len() >= 3 && contour.first() == contour.last() {
            builder.close();
        }
        has_contour = true;
    }

    if has_contour {
        builder.finish()
    } else {
        None
    }
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

fn uv_to_canvas_point(point: [f32; 2], width: u32, height: u32) -> OverlayPoint {
    [
        point[0] as f64 * width as f64,
        (1.0 - point[1] as f64) * height as f64,
    ]
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
    island_fill_enabled: bool,
    island_fill_opacity: f32,
    island_fill_padding_pixels: f32,
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
    let island_fill = if island_fill_enabled {
        Some(IslandFillConfig {
            enabled: true,
            opacity: island_fill_opacity,
            padding_pixels: island_fill_padding_pixels,
        })
    } else {
        None
    };

    let group_segment_indices = build_group_segment_indices(&original_segments, &group_segments);

    Ok(CompactPayload {
        styles,
        arrangement_input_segments: original_segments,
        arrangement_input_group_segments: group_segments,
        arrangement_input_group_segment_indices: group_segment_indices,
        point_positions: Arc::new(point_positions),
        polygons,
        padding_warning,
        island_fill,
    })
}

fn build_polygon_buffers_from_indexed_uvs(
    face_uv_counts: &[usize],
    face_uv_ids: &[usize],
    all_us: &[f32],
    all_vs: &[f32],
) -> Result<(Vec<usize>, Vec<f32>), BoxError> {
    if all_us.len() != all_vs.len() {
        return Err("UV coordinate arrays must have the same length".into());
    }
    if face_uv_counts.is_empty() {
        return Ok((vec![0], Vec::new()));
    }

    let mut polygon_offsets = Vec::with_capacity(face_uv_counts.len() + 1);
    let mut polygon_points = Vec::with_capacity(face_uv_ids.len() * 2);
    polygon_offsets.push(0);

    let uv_len = all_us.len();
    let mut uv_index = 0usize;
    let mut point_count = 0usize;
    for &face_uv_count in face_uv_counts {
        if face_uv_count == 0 {
            continue;
        }

        let next_uv_index = uv_index + face_uv_count;
        if next_uv_index > face_uv_ids.len() {
            return Err("face_uv_counts exceed face_uv_ids length".into());
        }
        let face_uv_ids = &face_uv_ids[uv_index..next_uv_index];

        let start_len = polygon_points.len();
        let mut deduped_point_count = 0usize;
        let mut first_u = 0.0f32;
        let mut first_v = 0.0f32;
        let mut previous_u = 0.0f32;
        let mut previous_v = 0.0f32;
        let mut has_previous = false;

        for &uv_id in face_uv_ids {
            if uv_id >= uv_len {
                return Err("face_uv_ids contain an out-of-range UV index".into());
            }
            let u = unsafe { *all_us.get_unchecked(uv_id) };
            let v = unsafe { *all_vs.get_unchecked(uv_id) };

            if has_previous && previous_u == u && previous_v == v {
                continue;
            }
            if deduped_point_count == 0 {
                first_u = u;
                first_v = v;
            }
            polygon_points.push(u);
            polygon_points.push(v);
            previous_u = u;
            previous_v = v;
            has_previous = true;
            deduped_point_count += 1;
        }

        if deduped_point_count >= 3 && previous_u == first_u && previous_v == first_v {
            polygon_points.truncate(polygon_points.len() - 2);
            deduped_point_count -= 1;
        }

        if deduped_point_count < 3 {
            polygon_points.truncate(start_len);
        } else {
            point_count += deduped_point_count;
            polygon_offsets.push(point_count);
        }

        uv_index = next_uv_index;
    }

    Ok((polygon_offsets, polygon_points))
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
    island_fill_enabled: bool,
    island_fill_opacity: f32,
    island_fill_padding_pixels: f32,
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
        island_fill_enabled,
        island_fill_opacity,
        island_fill_padding_pixels,
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

#[pyfunction(name = "build_polygon_buffers")]
fn build_polygon_buffers_py(
    face_uv_counts: Vec<usize>,
    face_uv_ids: Vec<usize>,
    all_us: Vec<f32>,
    all_vs: Vec<f32>,
) -> PyResult<(Vec<usize>, Vec<f32>)> {
    build_polygon_buffers_from_indexed_uvs(&face_uv_counts, &face_uv_ids, &all_us, &all_vs)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

#[pymodule(name = "_edge_drawer")]
fn _edge_drawer(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(draw_edges_py, module)?)?;
    module.add_function(wrap_pyfunction!(draw_edges_buffered_py, module)?)?;
    module.add_function(wrap_pyfunction!(build_polygon_buffers_py, module)?)?;
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

    fn payload_with_island_fill_json() -> String {
        r#"{
            "edges": [
                {
                    "outline_color": [255, 255, 255, 255],
                    "outline_width": 4.0,
                    "draw_outline": true,
                    "draw_internal": false,
                    "lines": [
                        {"uv1": [0.1, 0.1], "uv2": [0.5, 0.1]},
                        {"uv1": [0.5, 0.1], "uv2": [0.5, 0.5]},
                        {"uv1": [0.5, 0.5], "uv2": [0.1, 0.5]},
                        {"uv1": [0.1, 0.5], "uv2": [0.1, 0.1]},
                        {"uv1": [0.6, 0.1], "uv2": [0.9, 0.1]},
                        {"uv1": [0.9, 0.1], "uv2": [0.9, 0.4]},
                        {"uv1": [0.9, 0.4], "uv2": [0.6, 0.4]},
                        {"uv1": [0.6, 0.4], "uv2": [0.6, 0.1]}
                    ]
                }
            ],
            "polygons": [
                {"points": [[0.1, 0.1], [0.5, 0.1], [0.5, 0.5], [0.1, 0.5]]},
                {"points": [[0.6, 0.1], [0.9, 0.1], [0.9, 0.4], [0.6, 0.4]]}
            ],
            "island_fill": {
                "enabled": true,
                "opacity": 0.25,
                "padding_pixels": 3.0
            }
        }"#
        .to_string()
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
    fn build_polygon_buffers_dedupes_consecutive_points_and_closing_duplicate() {
        let (offsets, points) = build_polygon_buffers_from_indexed_uvs(
            &[5],
            &[0, 1, 1, 2, 0],
            &[0.0, 1.0, 1.0],
            &[0.0, 0.0, 1.0],
        )
        .unwrap();

        assert_eq!(offsets, vec![0, 3]);
        assert_eq!(points, vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn build_polygon_buffers_discards_degenerate_faces() {
        let (offsets, points) = build_polygon_buffers_from_indexed_uvs(
            &[2, 3],
            &[0, 1, 0, 1, 2],
            &[0.0, 1.0, 1.0],
            &[0.0, 0.0, 1.0],
        )
        .unwrap();

        assert_eq!(offsets, vec![0, 3]);
        assert_eq!(points, vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
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
        assert!(payload.island_fill.is_none());
        let warning = payload.padding_warning.unwrap();
        assert!(warning.enabled);
        assert_eq!(warning.padding_pixels, 8.0);
        assert_eq!(warning.warning_width, 6.0);
        assert_eq!(warning.warning_color, [255, 64, 64, 255]);
    }

    #[test]
    fn test_parse_drawer_payload_island_fill() {
        let payload = parse_drawer_payload(&payload_with_island_fill_json()).unwrap();
        let island_fill = payload.island_fill.unwrap();
        assert!(island_fill.enabled);
        assert_eq!(island_fill.opacity, 0.25);
        assert_eq!(island_fill.padding_pixels, 3.0);
    }

    #[test]
    fn test_parse_drawer_payload_ignores_null_optional_configs() {
        let payload = parse_drawer_payload(
            r#"{
                "edges": [],
                "polygons": [],
                "padding_warning": null,
                "island_fill": null
            }"#,
        )
        .unwrap();

        assert!(payload.padding_warning.is_none());
        assert!(payload.island_fill.is_none());
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
        let prepared = prepare_drawing(&edges, &[], 128, 128, None, None);
        assert_eq!(prepared.groups.len(), 2);
        assert_eq!(prepared.groups[0].line_width, 3.0);
        assert_eq!(prepared.groups[1].line_width, 6.0);
    }

    #[test]
    fn test_island_fill_groups_separated_polygons() {
        let payload = parse_drawer_payload(&payload_with_island_fill_json()).unwrap();
        let prepared = prepare_drawing(
            &payload.edges,
            &payload.polygons,
            128,
            128,
            None,
            payload.island_fill.as_ref(),
        );

        assert_eq!(prepared.fills.len(), 2);
        assert_eq!(prepared.fills[0].fill_color[3], 64);
        assert_eq!(prepared.fills[0].shapes.len(), 1);
        assert_eq!(prepared.fills[1].shapes.len(), 1);
    }

    #[test]
    fn test_island_fill_keeps_edge_connected_polygons_one_island() {
        let polygons = vec![
            Polygon {
                points: vec![[0.1, 0.1], [0.5, 0.1], [0.5, 0.5], [0.1, 0.5]],
            },
            Polygon {
                points: vec![[0.5, 0.1], [0.9, 0.1], [0.9, 0.5], [0.5, 0.5]],
            },
        ];
        let fills = build_island_fills(
            &polygons,
            Some(&IslandFillConfig {
                enabled: true,
                opacity: 0.25,
                padding_pixels: 0.0,
            }),
            128,
            128,
        );

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].shapes.len(), 1);
        assert_eq!(fills[0].shapes[0].len(), 1);
        assert!(!fills[0].shapes[0][0].windows(2).any(|points| {
            canonical_segment(points[0], points[1])
                == canonical_segment(quantize_point([0.5, 0.1]), quantize_point([0.5, 0.5]))
        }));
    }

    #[test]
    fn test_island_fill_unions_polygon_layout_with_t_junctions() {
        let polygons = vec![
            Polygon {
                points: vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.4], [0.1, 0.4]],
            },
            Polygon {
                points: vec![[0.4, 0.4], [0.6, 0.4], [0.6, 0.8], [0.4, 0.8]],
            },
        ];
        let fills = build_island_fills(
            &polygons,
            Some(&IslandFillConfig {
                enabled: true,
                opacity: 0.25,
                padding_pixels: 0.0,
            }),
            128,
            128,
        );

        assert_eq!(fills.len(), 1);
        assert!(point_in_fill_shapes([0.5, 0.7], &fills[0].shapes));
    }

    #[test]
    fn test_island_fill_preserves_holes_from_polygon_union() {
        let polygons = vec![
            Polygon {
                points: vec![[0.1, 0.1], [0.9, 0.1], [0.9, 0.3], [0.1, 0.3]],
            },
            Polygon {
                points: vec![[0.1, 0.7], [0.9, 0.7], [0.9, 0.9], [0.1, 0.9]],
            },
            Polygon {
                points: vec![[0.1, 0.3], [0.3, 0.3], [0.3, 0.7], [0.1, 0.7]],
            },
            Polygon {
                points: vec![[0.7, 0.3], [0.9, 0.3], [0.9, 0.7], [0.7, 0.7]],
            },
        ];
        let fills = build_island_fills(
            &polygons,
            Some(&IslandFillConfig {
                enabled: true,
                opacity: 0.25,
                padding_pixels: 0.0,
            }),
            128,
            128,
        );

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].shapes[0].len(), 2);
        assert!(point_in_fill_shapes([0.2, 0.2], &fills[0].shapes));
        assert!(!point_in_fill_shapes([0.5, 0.5], &fills[0].shapes));
    }

    #[test]
    fn test_island_fill_padding_is_kept_for_raster_distance_field() {
        let polygons = vec![Polygon {
            points: vec![[0.2, 0.2], [0.4, 0.2], [0.4, 0.4], [0.2, 0.4]],
        }];
        let fills = build_island_fills(
            &polygons,
            Some(&IslandFillConfig {
                enabled: true,
                opacity: 0.25,
                padding_pixels: 10.0,
            }),
            100,
            100,
        );

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].padding_pixels, 10.0);
        let (min_x, max_x, min_y, max_y) = fill_shapes_bounds(&fills[0].shapes);
        assert_eq!(min_x, quantize_point([0.2, 0.2]).x);
        assert_eq!(max_x, quantize_point([0.4, 0.4]).x);
        assert_eq!(min_y, quantize_point([0.2, 0.2]).y);
        assert_eq!(max_y, quantize_point([0.4, 0.4]).y);
    }

    #[test]
    fn test_raster_island_fill_padding_uses_nearest_island_owner() {
        let polygons = vec![
            Polygon {
                points: vec![[0.2, 0.2], [0.4, 0.2], [0.4, 0.4], [0.2, 0.4]],
            },
            Polygon {
                points: vec![[0.5, 0.2], [0.7, 0.2], [0.7, 0.4], [0.5, 0.4]],
            },
        ];
        let prepared = prepare_drawing(
            &[],
            &polygons,
            100,
            100,
            None,
            Some(&IslandFillConfig {
                enabled: true,
                opacity: 0.25,
                padding_pixels: 10.0,
            }),
        );
        let pixmap = draw_edges_raster(&prepared, 100, 100).unwrap();

        assert_left_island_pixel(&pixmap, 44, 70);
        assert_right_island_pixel(&pixmap, 46, 70);
        assert_right_island_pixel(&pixmap, 45, 70);
    }

    fn fill_shapes_bounds(shapes: &[FillShape]) -> (i64, i64, i64, i64) {
        let mut min_x = i64::MAX;
        let mut max_x = i64::MIN;
        let mut min_y = i64::MAX;
        let mut max_y = i64::MIN;
        for shape in shapes {
            for contour in shape {
                for point in contour {
                    min_x = min_x.min(point.x);
                    max_x = max_x.max(point.x);
                    min_y = min_y.min(point.y);
                    max_y = max_y.max(point.y);
                }
            }
        }
        (min_x, max_x, min_y, max_y)
    }

    fn point_in_fill_shapes(point: [f32; 2], shapes: &[FillShape]) -> bool {
        shapes.iter().any(|shape| {
            shape.iter().fold(false, |inside, contour| {
                let points = contour
                    .iter()
                    .map(|vertex| {
                        [
                            vertex.x as f32 / QUANTIZE_SCALE,
                            vertex.y as f32 / QUANTIZE_SCALE,
                        ]
                    })
                    .collect::<Vec<_>>();
                inside ^ point_in_polygon_points(point, &points)
            })
        })
    }

    fn assert_left_island_pixel(pixmap: &Pixmap, x: u32, y: u32) {
        let color = pixel_color_u8(pixmap, x, y);
        assert_eq!(color[3], 64);
        assert!(
            color[2] > color[0],
            "expected blue island color, got {color:?}"
        );
    }

    fn assert_right_island_pixel(pixmap: &Pixmap, x: u32, y: u32) {
        let color = pixel_color_u8(pixmap, x, y);
        assert_eq!(color[3], 64);
        assert!(
            color[0] > color[2],
            "expected orange island color, got {color:?}"
        );
    }

    fn pixel_color_u8(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let color = pixmap.pixels()[pixel_index(x, y, pixmap.width())].demultiply();
        [color.red(), color.green(), color.blue(), color.alpha()]
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
        assert!(visible.contains(
            &canonical_segment(quantize_point([0.1, 0.2]), quantize_point([0.3, 0.2])).unwrap()
        ));
        assert!(visible.contains(
            &canonical_segment(quantize_point([0.3, 0.2]), quantize_point([0.5, 0.2])).unwrap()
        ));
        assert!(visible.contains(
            &canonical_segment(quantize_point([0.5, 0.2]), quantize_point([0.7, 0.2])).unwrap()
        ));
        assert!(!visible.contains(
            &canonical_segment(quantize_point([0.3, 0.2]), quantize_point([0.3, 0.8])).unwrap()
        ));
        assert!(!visible.contains(
            &canonical_segment(quantize_point([0.5, 0.2]), quantize_point([0.5, 0.8])).unwrap()
        ));
    }

    #[test]
    fn test_point_in_polygons_treats_overlaps_as_union() {
        let polygons = overlapping_polygons();
        let polygon_index = build_polygon_index(&polygons);
        assert!(point_in_polygons([0.5, 0.5], &polygon_index, None));
        assert!(!point_in_polygons([0.95, 0.95], &polygon_index, None));
        assert_eq!(
            point_in_polygons_bruteforce([0.5, 0.5], &polygons),
            point_in_polygons([0.5, 0.5], &polygon_index, None)
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
        assert!(paths
            .iter()
            .any(|path| path == &vec![a, b] || path == &vec![b, a]));
        assert!(paths
            .iter()
            .any(|path| path == &vec![b, c] || path == &vec![c, b]));
        assert!(paths
            .iter()
            .any(|path| path == &vec![b, d] || path == &vec![d, b]));
    }

    #[test]
    fn test_dedup_sorted_segments_in_place_keeps_sorted_unique_segments() {
        let segment_a =
            canonical_segment(quantize_point([0.0, 0.0]), quantize_point([1.0, 0.0])).unwrap();
        let segment_b =
            canonical_segment(quantize_point([1.0, 0.0]), quantize_point([2.0, 0.0])).unwrap();
        let segment_c =
            canonical_segment(quantize_point([2.0, 0.0]), quantize_point([3.0, 0.0])).unwrap();
        let mut segments = vec![
            segment_a, segment_a, segment_b, segment_b, segment_c, segment_c,
        ];

        dedup_sorted_segments_in_place(&mut segments);

        assert_eq!(segments, vec![segment_a, segment_b, segment_c]);
    }

    fn arrangement_from_segments(segments: &[([f32; 2], [f32; 2])]) -> SegmentArrangement {
        let original_segments = segments
            .iter()
            .map(|(start, end)| {
                canonical_segment(quantize_point(*start), quantize_point(*end)).unwrap()
            })
            .collect::<Vec<_>>();
        let mut point_positions = HashMap::new();
        for (start, end) in segments {
            point_positions
                .entry(quantize_point(*start))
                .or_insert(*start);
            point_positions.entry(quantize_point(*end)).or_insert(*end);
        }
        let group_segments = vec![original_segments.clone()];
        let group_segment_indices = vec![(0..original_segments.len()).collect::<Vec<_>>()];
        build_segment_arrangement_from_parts(
            original_segments,
            Arc::new(point_positions),
            &group_segments,
            &group_segment_indices,
        )
    }

    #[test]
    fn test_arrangement_shared_endpoint_does_not_split() {
        let arrangement =
            arrangement_from_segments(&[([0.0, 0.0], [1.0, 0.0]), ([1.0, 0.0], [1.0, 1.0])]);
        assert_eq!(arrangement.segments.len(), 2);
        assert_eq!(arrangement.group_segments.len(), 1);
        assert_eq!(arrangement.group_segments[0].len(), 2);
    }

    #[test]
    fn test_arrangement_t_junction_splits_only_trunk() {
        let arrangement =
            arrangement_from_segments(&[([0.0, 0.0], [1.0, 0.0]), ([0.5, 0.0], [0.5, 1.0])]);
        assert_eq!(arrangement.segments.len(), 3);
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([0.0, 0.0]), quantize_point([0.5, 0.0]),).unwrap()
        ));
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([0.5, 0.0]), quantize_point([1.0, 0.0]),).unwrap()
        ));
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([0.5, 0.0]), quantize_point([0.5, 1.0]),).unwrap()
        ));
    }

    #[test]
    fn test_arrangement_crossing_splits_both_segments() {
        let arrangement =
            arrangement_from_segments(&[([0.0, 0.0], [1.0, 1.0]), ([0.0, 1.0], [1.0, 0.0])]);
        assert_eq!(arrangement.segments.len(), 4);
        let center = quantize_point([0.5, 0.5]);
        assert!(arrangement
            .segments
            .contains(&canonical_segment(quantize_point([0.0, 0.0]), center,).unwrap()));
        assert!(arrangement
            .segments
            .contains(&canonical_segment(center, quantize_point([1.0, 1.0]),).unwrap()));
        assert!(arrangement
            .segments
            .contains(&canonical_segment(quantize_point([0.0, 1.0]), center,).unwrap()));
        assert!(arrangement
            .segments
            .contains(&canonical_segment(center, quantize_point([1.0, 0.0]),).unwrap()));
    }

    #[test]
    fn test_arrangement_colinear_endpoint_touch_does_not_split() {
        let arrangement =
            arrangement_from_segments(&[([0.0, 0.0], [1.0, 0.0]), ([1.0, 0.0], [2.0, 0.0])]);
        assert_eq!(arrangement.segments.len(), 2);
    }

    #[test]
    fn test_arrangement_colinear_overlap_splits_internal_boundaries_only() {
        let arrangement =
            arrangement_from_segments(&[([0.0, 0.0], [2.0, 0.0]), ([1.0, 0.0], [3.0, 0.0])]);
        assert_eq!(arrangement.segments.len(), 3);
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([0.0, 0.0]), quantize_point([1.0, 0.0]),).unwrap()
        ));
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([1.0, 0.0]), quantize_point([2.0, 0.0]),).unwrap()
        ));
        assert!(arrangement.segments.contains(
            &canonical_segment(quantize_point([2.0, 0.0]), quantize_point([3.0, 0.0]),).unwrap()
        ));
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
    fn test_draw_png_to_path_with_island_fill() {
        let dir = tempdir().unwrap();
        let image_path = dir.path().join("filled_edge.png");

        draw_to_path(
            image_path.as_path(),
            128,
            128,
            &payload_with_island_fill_json(),
        )
        .unwrap();

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
    fn test_draw_svg_to_path_with_island_fill_writes_fill_before_stroke() {
        let dir = tempdir().unwrap();
        let image_path = dir.path().join("filled_edge.svg");

        draw_to_path(
            image_path.as_path(),
            128,
            128,
            &payload_with_island_fill_json(),
        )
        .unwrap();

        let contents = fs::read_to_string(&image_path).unwrap();
        let fill_index = contents.find("fill-opacity").unwrap();
        let stroke_index = contents.find("stroke-linejoin").unwrap();
        assert!(fill_index < stroke_index);
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
