use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_antialiased_line_segment_mut, draw_polygon_mut};
use imageproc::pixelops::interpolate;
use imageproc::point::Point;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::Deserialize;
use svg::node::element::path::Data;
use svg::node::element::Path as SvgPath;
use svg::Document;

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug)]
pub struct Config {
    pub image_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub edges: Vec<Edges>,
}

#[derive(Debug, Deserialize)]
pub struct Edges {
    line_width: f32,
    line_color: [u8; 4],
    lines: Vec<Edge>,
}

#[derive(Debug, Deserialize)]
pub struct Edge {
    uv1: [f32; 2],
    uv2: [f32; 2],
}

pub fn parse_edges_json(edges_json: &str) -> Result<Vec<Edges>, BoxError> {
    let edges_data: serde_json::Value = serde_json::from_str(edges_json)?;
    let edge_list = edges_data
        .as_array()
        .ok_or_else(|| "Expected 'edges' to be an array".to_string())?;

    edge_list
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Edges>, _>>()
        .map_err(Into::into)
}

pub fn load_edges_input(edges_arg: &str) -> Result<Vec<Edges>, BoxError> {
    if Path::new(edges_arg).exists() {
        let file_contents = fs::read_to_string(edges_arg)?;
        parse_edges_json(&file_contents)
    } else {
        parse_edges_json(edges_arg)
    }
}

pub fn draw_to_path(
    image_path: &Path,
    width: u32,
    height: u32,
    edges_json: &str,
) -> Result<(), BoxError> {
    let edges = parse_edges_json(edges_json)?;
    draw_to_path_from_edges(image_path, width, height, &edges)
}

pub fn draw_to_path_from_input(
    image_path: &Path,
    width: u32,
    height: u32,
    edges_input: &str,
) -> Result<(), BoxError> {
    let edges = load_edges_input(edges_input)?;
    draw_to_path_from_edges(image_path, width, height, &edges)
}

pub fn draw_to_path_from_edges(
    image_path: &Path,
    width: u32,
    height: u32,
    edges: &[Edges],
) -> Result<(), BoxError> {
    if image_path.extension().and_then(|s| s.to_str()) == Some("svg") {
        let document = draw_edges_svg(edges, width, height);
        save_svg(&document, image_path)?;
    } else {
        let mut img = RgbaImage::new(width, height);
        draw_edges_raster(&mut img, edges);
        save_image(&img, image_path)?;
    }

    Ok(())
}

fn draw_edges_svg(edges: &[Edges], width: u32, height: u32) -> Document {
    let mut document = Document::new().set("viewBox", (0, 0, width, height));

    for edge in edges {
        for line in &edge.lines {
            let u1 = line.uv1[0] * width as f32;
            let v1 = (1.0 - line.uv1[1]) * height as f32;
            let u2 = line.uv2[0] * width as f32;
            let v2 = (1.0 - line.uv2[1]) * height as f32;

            let data = Data::new()
                .move_to((u1 as f64, v1 as f64))
                .line_to((u2 as f64, v2 as f64));

            let color = format!(
                "rgb({}, {}, {})",
                edge.line_color[0], edge.line_color[1], edge.line_color[2]
            );

            let path = SvgPath::new()
                .set("fill", "none")
                .set("stroke", color)
                .set("stroke-width", edge.line_width)
                .set("stroke-linecap", "round")
                .set("d", data);

            document = document.add(path);
        }
    }

    document
}

fn draw_edges_raster(img: &mut RgbaImage, edges: &[Edges]) {
    for edge in edges {
        for line in &edge.lines {
            let u1 = line.uv1[0] * img.width() as f32;
            let v1 = (1.0 - line.uv1[1]) * img.height() as f32;
            let u2 = line.uv2[0] * img.width() as f32;
            let v2 = (1.0 - line.uv2[1]) * img.height() as f32;

            let p1 = (u1, v1);
            let p2 = (u2, v2);
            let color = Rgba([
                edge.line_color[0],
                edge.line_color[1],
                edge.line_color[2],
                edge.line_color[3],
            ]);

            if edge.line_width <= 1.0 {
                draw_antialiased_line_segment_mut(
                    img,
                    (u1 as i32, v1 as i32),
                    (u2 as i32, v2 as i32),
                    color,
                    interpolate,
                );
            } else {
                draw_wide_line_segment_mut(img, p1, p2, edge.line_width, color);
            }
        }
    }
}

fn draw_wide_line_segment_mut(
    img: &mut RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    line_width: f32,
    line_color: Rgba<u8>,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let norm = (dx * dx + dy * dy).sqrt();

    if norm == 0.0 {
        return;
    }

    let perp_dx = -dy / norm * line_width / 2.0;
    let perp_dy = dx / norm * line_width / 2.0;

    let points = [
        Point::new((start.0 + perp_dx) as i32, (start.1 + perp_dy) as i32),
        Point::new((end.0 + perp_dx) as i32, (end.1 + perp_dy) as i32),
        Point::new((end.0 - perp_dx) as i32, (end.1 - perp_dy) as i32),
        Point::new((start.0 - perp_dx) as i32, (start.1 - perp_dy) as i32),
    ];

    draw_polygon_mut(img, &points, line_color);
}

fn save_image(img: &RgbaImage, image_path: &Path) -> Result<(), BoxError> {
    img.save(image_path)?;
    Ok(())
}

fn save_svg(document: &Document, image_path: &Path) -> Result<(), BoxError> {
    svg::save(image_path, document)?;
    Ok(())
}

#[pyfunction(name = "draw_edges")]
fn draw_edges_py(image_path: &str, width: u32, height: u32, edges_json: &str) -> PyResult<()> {
    draw_to_path(Path::new(image_path), width, height, edges_json)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

#[pymodule(name = "_edge_drawer")]
fn _edge_drawer(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(draw_edges_py, module)?)?;
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
        "line_color": [255, 0, 0, 255],
        "line_width": 2.0,
        "lines": [
          {"uv1": [0.1, 0.1], "uv2": [0.8, 0.8]},
          {"uv1": [0.2, 0.8], "uv2": [0.8, 0.2]}
        ]
      }
    ]
    "#;

    #[test]
    fn test_parse_edges_json_valid_data() {
        let edges = parse_edges_json(VALID_JSON).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].line_width, 2.0);
        assert_eq!(edges[0].line_color, [255, 0, 0, 255]);
        assert_eq!(edges[0].lines.len(), 2);
        assert_eq!(edges[0].lines[0].uv1, [0.1, 0.1]);
        assert_eq!(edges[0].lines[0].uv2, [0.8, 0.8]);
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

        draw_to_path(image_path.as_path(), 128, 128, VALID_JSON).unwrap();

        let contents = fs::read_to_string(&image_path).unwrap();
        assert!(contents.contains("<svg"));
        assert!(contents.contains("stroke-width=\"2\""));
    }

    #[test]
    fn test_load_edges_input_from_file() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("edges.json");
        fs::write(&json_path, VALID_JSON).unwrap();

        let edges = load_edges_input(json_path.to_str().unwrap()).unwrap();
        assert_eq!(edges.len(), 1);
    }
}
