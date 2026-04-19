use std::path::PathBuf;

use clap::{Arg, Command};
use edge_drawer::{draw_to_path_from_edges, Config, load_edges_input};

fn parse_arguments() -> Config {
    let matches = Command::new("UV Image Edge Drawer")
        .version("1.0")
        .about("Draws edges on an image based on JSON input")
        .arg(
            Arg::new("IMAGE")
                .help("Sets the output image file path")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("WIDTH")
                .help("Sets the output image width")
                .required(true)
                .index(2),
        )
        .arg(
            Arg::new("HEIGHT")
                .help("Sets the output image height")
                .required(true)
                .index(3),
        )
        .arg(
            Arg::new("EDGES")
                .help("Sets the JSON string for edge data")
                .required(true)
                .index(4),
        )
        .get_matches();

    let image_path = PathBuf::from(matches.get_one::<String>("IMAGE").unwrap());
    let width = matches
        .get_one::<String>("WIDTH")
        .unwrap()
        .parse()
        .expect("Invalid WIDTH");
    let height = matches
        .get_one::<String>("HEIGHT")
        .unwrap()
        .parse()
        .expect("Invalid HEIGHT");
    let edges_input = matches.get_one::<String>("EDGES").unwrap();
    let edges = load_edges_input(edges_input).expect("Failed to parse edge data");

    Config {
        image_path,
        width,
        height,
        edges,
    }
}

fn main() {
    let config = parse_arguments();
    draw_to_path_from_edges(
        config.image_path.as_path(),
        config.width,
        config.height,
        &config.edges,
    )
    .expect("Failed to draw edges");
}
