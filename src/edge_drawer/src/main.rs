extern crate image;
extern crate imageproc;

use std::fs;
use image::{Rgba, RgbaImage};
use imageproc::drawing::draw_polygon_mut;
use imageproc::point::Point;
use std::path::Path;
use clap::{App, Arg};
// use serde_json::Value;
use serde::Deserialize;
use std::path::PathBuf;

struct Config {
    image_path: PathBuf,
    edges: Vec<Edges>,
    output_path: PathBuf,
}


#[derive(Debug, Deserialize)]
struct Edges {
    line_width: f32,
    line_color: [u8; 4],
    lines: Vec<Edge>,
}


#[derive(Debug, Deserialize)]
struct Edge {
    // mesh_name: String,
    // edge_id: usize,
    uv1: [f32; 2],
    uv2: [f32; 2],
}

fn parse_edges_json(edges_json: &str) -> Vec<Edges> {

    println!("{}", edges_json);
    let edges_data: serde_json::Value = serde_json::from_str(edges_json)
        .expect("Failed to parse edge data");

    edges_data.as_array()
        .expect("Expected 'edges' to be an array")
        .iter()
        .map(|edge| serde_json::from_value(edge.clone())
            .expect("Error parsing edge"))
        .collect()
}


fn parse_arguments() -> Config {
    let matches = App::new("UV Image Edge Drawer")
        .version("1.0")
        .about("Draws edges on an image based on JSON input")
        .arg(Arg::with_name("IMAGE")
            .help("Sets the input image file path")
            .required(true)
            .index(1))
        .arg(Arg::with_name("EDGES")
            .help("Sets the JSON string for edge data")
            .required(true)
            .index(2))
        .arg(Arg::with_name("output")
            .help("Sets the output image file path")
            .short('o')
            .long("output")
            .takes_value(true)
            .required(false))
        .get_matches();

    let image_path: PathBuf = matches.value_of("IMAGE").unwrap().into();

    let edges = if let Some(edges_arg) = matches.value_of("EDGES") {
        if Path::new(edges_arg).exists() {
            let file_contents = fs::read_to_string(edges_arg)
                .expect("Failed to read edge file");
            parse_edges_json(&file_contents)
        } else {
            // 直接JSON文字列として解析
            parse_edges_json(edges_arg)
        }
    } else {
        panic!("No edges data provided");
    };

    let output_path = if let Some(output) = matches.value_of("output") {
        PathBuf::from(output)
    } else {
        generate_output_path(&image_path)
    };

    Config {
        image_path,
        edges,
        output_path,
    }
}

fn generate_output_path(image_path: &Path) -> PathBuf {
    let mut output_path = image_path.with_file_name(
        image_path.file_stem().unwrap().to_str().unwrap().to_owned() + "_edges"
    );

    if let Some(ext) = image_path.extension() {
        output_path.set_extension(ext);
    }

    output_path
}


fn load_image(image_path: &Path) -> RgbaImage {
    image::open(image_path)
        .expect("Failed to load image")
        .to_rgba8()
}


fn draw_edges(img: &mut RgbaImage, edges: &[Edges]) {

    for edge in edges {

        for line in edge.lines.iter() {

            // UV座標をピクセル座標に変換
            let u1 = line.uv1[0] * img.width() as f32;
            let v1 = (1.0 - line.uv1[1]) * img.height() as f32;
            let u2 = line.uv2[0] * img.width() as f32;
            let v2 = (1.0 - line.uv2[1]) * img.height() as f32;

            let p1 = (u1 as f32, v1 as f32);
            let p2 = (u2 as f32, v2 as f32);

            let color = Rgba([edge.line_color[0], edge.line_color[1], edge.line_color[2], edge.line_color[3]]);
            draw_wide_line_segment_mut(img, p1, p2, edge.line_width, color);
        }
    }
}

fn draw_wide_line_segment_mut(
    img: &mut RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    line_width: f32,
    line_color: Rgba<u8>
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;

    let norm = ((dx*dx + dy*dy) as f32).sqrt();
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


fn save_image(img: &RgbaImage, image_path: &PathBuf) {

    img.save(image_path).expect("Failed to save image");
}


fn main() {
    let config = parse_arguments();
    let mut img = load_image(&config.image_path);
    draw_edges(&mut img, &config.edges);
    save_image(&img, &config.output_path);
}

// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edges_json_valid_data() {
        let json_str = r#"
        [
            {"uv1": [0.1, 0.1], "uv2": [0.2,  0.2]},
            {"uv1": [0.3, 0.3], "uv2": [0.4,  0.4]}
        ]
        "#;

        let edges = parse_edges_json(json_str);
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].uv1[0], 0.1);
        assert_eq!(edges[0].uv1[1], 0.1);
        assert_eq!(edges[0].uv2[0], 0.2);
        assert_eq!(edges[0].uv2[1], 0.2);
    }

    #[test]
    #[should_panic(expected = "Failed to parse edge data")]
    fn test_parse_edges_json_invalid_data() {
        let invalid_json_str = r#"
        [
            {"u1": "0.1" "v1": 0.1, "u2": 0.2, "v2": 0.2}
        ]
        "#;

        parse_edges_json(invalid_json_str);
    }

    #[test]
    #[should_panic(expected = "Error parsing edge")]
    fn test_parse_edges_json_invalid_data2() {
        let invalid_json_str2 = r#"
        [
            {"u1": "invalid", "v1": 0.1, "u2": 0.2, "v2": 0.2}
        ]
        "#;
        parse_edges_json(invalid_json_str2);
    }

    #[test]
    #[should_panic(expected = "Error parsing edge")]
    fn test_parse_edges_json_missing_fields() {
        let missing_fields_json_str = r#"
        [
            { "uv1": [0.1, 0.1]}
        ]
        "#;

        parse_edges_json(missing_fields_json_str);
    }
}
