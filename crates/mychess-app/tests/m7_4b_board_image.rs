//! M7-4b contract: the SVG and the PNG of a position (spec
//! `docs/specs/m7-4-exportacao.md`, rules 26-29, D3723-D3726, criteria 12
//! and 13).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m7_4b_common;

use m7_4b_common::*;
use mychess_app::board_image::SIZE_RANGE;
use mychess_core::{BoardMark, MarkKind, Position, Square};

fn image(size: u32) -> BoardImage {
    BoardImage::new(Position::new(), real_pieces("cburnett"), size)
}

/// Decodes PNG bytes into (width, height, RGBA pixels).
fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let pixmap = resvg::tiny_skia::Pixmap::decode_png(bytes).expect("a decodable PNG");
    (pixmap.width(), pixmap.height(), pixmap.data().to_vec())
}

/// The RGBA of the pixel (x, y), un-premultiplied for opaque pixels.
fn pixel(data: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    [data[at], data[at + 1], data[at + 2], data[at + 3]]
}

#[test]
fn criterion_12_the_start_position_svg_has_64_squares_32_pieces_16_coordinates() {
    let svg = board_svg(&image(512));
    assert_eq!(svg.matches("class=\"sq\"").count(), 64);
    assert_eq!(svg.matches("<use ").count(), 32);
    assert_eq!(svg.matches("class=\"co\"").count(), 16);
    assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(svg.contains("viewBox=\"0 0 800 800\""));
    assert!(svg.contains("font-family=\"Noto Sans, sans-serif\""));
    assert!(svg.contains("font-size=\"12\""), "12% of a square");
}

#[test]
fn criterion_12_flipping_swaps_the_coordinates_and_the_pieces() {
    let normal = board_svg(&image(512));
    let mut img = image(512);
    img.flip = true;
    let flipped = board_svg(&img);
    // The white king: bottom of file e, then top of file d when flipped.
    assert!(normal.contains("href=\"#wK\" x=\"400\" y=\"700\""));
    assert!(flipped.contains("href=\"#wK\" x=\"300\" y=\"0\""));
    // The label on the bottom-left corner: `a`/`1`, then `h`/`8`.
    assert!(normal.contains(">a</text>") && normal.contains(">1</text>"));
    assert!(flipped.contains(">h</text>") && flipped.contains(">8</text>"));
}

#[test]
fn criterion_12_last_move_and_arrows_are_drawn_and_the_output_is_deterministic() {
    let mut img = image(512);
    img.last_move = Some((Square::E2, Square::E4));
    img.marks = vec![
        BoardMark::arrow(Square::G1, Square::F3, "green"),
        BoardMark::square(MarkKind::Full, Square::D4, "#ff0000"),
    ];
    let first = board_svg(&img);
    assert_eq!(first.matches("class=\"hl\"").count(), 2);
    assert_eq!(first.matches("class=\"ar\"").count(), 2);
    assert_eq!(first.matches("class=\"mk\"").count(), 1);
    assert_eq!(first, board_svg(&img));
}

#[test]
fn criterion_12_without_coordinates_there_are_no_labels() {
    let mut img = image(512);
    img.coordinates = false;
    assert_eq!(board_svg(&img).matches("class=\"co\"").count(), 0);
}

#[test]
fn criterion_12_namespacing_prefixes_ids_and_their_references() {
    let king = r##"<svg><defs><linearGradient id="a"/></defs><path fill="url(#a)"/><use href="#a"/></svg>"##;
    let queen = r#"<svg><linearGradient id="a"/><path stroke="url(#a)"/></svg>"#;
    let k = namespace_svg_ids(king, "wK-");
    let q = namespace_svg_ids(queen, "bQ-");
    assert!(k.contains("id=\"wK-a\"") && k.contains("url(#wK-a)") && k.contains("href=\"#wK-a\""));
    assert!(q.contains("id=\"bQ-a\"") && q.contains("url(#bQ-a)"));
    let plain = r#"<svg><path d="M0 0"/></svg>"#;
    assert_eq!(namespace_svg_ids(plain, "x-"), plain);
}

#[test]
fn d3726_pieces_of_one_set_never_share_an_id() {
    // Real sets repeat short ids across pieces; the board must not.
    for set in ["cburnett", "merida", "chessnut", "celtic"] {
        let mut img = BoardImage::new(Position::new(), real_pieces(set), 256);
        img.id_prefix = "p1-".to_owned();
        let svg = board_svg(&img);
        let mut ids: Vec<&str> = svg
            .match_indices(" id=\"")
            .map(|(i, _)| svg[i + 5..].split('"').next().unwrap())
            .collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "{set}: duplicate ids");
        assert!(total >= 12, "{set}: one symbol per piece kind");
    }
}

#[test]
fn criterion_13_the_png_has_the_requested_size_and_the_dark_color_on_a1() {
    // An empty a1: no piece in the way. a1 is a dark square.
    let mut img = image(512);
    img.position = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    img.coordinates = false;
    let bytes = board_png(&img, false, &font()).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h, data) = decode(&bytes);
    assert_eq!((w, h), (512, 512));
    // Centre of a1 (bottom left square, 64 px).
    assert_eq!(pixel(&data, w, 32, 512 - 32), [0xb5, 0x88, 0x63, 255]);
    // Centre of b1 is light.
    assert_eq!(pixel(&data, w, 96, 512 - 32), [0xf0, 0xd9, 0xb5, 255]);
}

#[test]
fn criterion_13_a_piece_changes_the_pixels_of_its_square() {
    let mut img = image(512);
    img.coordinates = false;
    let bytes = board_png(&img, false, &font()).unwrap();
    let (w, _, with_pieces) = decode(&bytes);
    let mut empty = image(512);
    empty.coordinates = false;
    empty.position = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let (_, _, without) = decode(&board_png(&empty, false, &font()).unwrap());
    // a1 holds a white rook: its drawing paints inside the square, while
    // the empty board shows the plain square color there.
    let square = |data: &[u8]| -> Vec<[u8; 4]> {
        (0..64)
            .flat_map(|dy| (0..64).map(move |dx| (dx, 448 + dy)))
            .map(|(x, y)| pixel(data, w, x, y))
            .collect()
    };
    let (with, plain) = (square(&with_pieces), square(&without));
    assert!(
        plain.iter().all(|p| p == &[0xb5, 0x88, 0x63, 255]),
        "empty a1 is plain"
    );
    let changed = with.iter().zip(&plain).filter(|(a, b)| a != b).count();
    assert!(changed > 400, "the rook paints {changed} pixels of a1");
    // And the rook has black outline pixels (the set draws them).
    assert!(
        with.iter()
            .any(|p| p[0] < 40 && p[1] < 40 && p[2] < 40 && p[3] == 255)
    );
}

#[test]
fn criterion_13_a_transparent_png_has_alpha_zero_on_empty_squares() {
    let mut img = image(512);
    img.position = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    img.coordinates = false;
    let (w, _, data) = decode(&board_png(&img, true, &font()).unwrap());
    assert_eq!(pixel(&data, w, 32, 512 - 32)[3], 0, "empty a1");
    assert_eq!(pixel(&data, w, 450, 100)[3], 0, "empty g7");
    // A piece is still there: e1 (centre of the 5th column of the bottom row).
    let king = pixel(&data, w, 4 * 64 + 32, 512 - 32);
    let any_opaque = (0..64)
        .flat_map(|dy| (0..64).map(move |dx| (4 * 64 + dx, 448 + dy)))
        .any(|(x, y)| pixel(&data, w, x, y)[3] > 0);
    assert!(any_opaque, "the king paints its square (centre {king:?})");
}

#[test]
fn criterion_13_the_margin_widens_the_png() {
    let mut img = image(256);
    img.margin_px = 20;
    let (w, h, data) = decode(&board_png(&img, false, &font()).unwrap());
    assert_eq!((w, h), (296, 296));
    assert_eq!(
        pixel(&data, w, 5, 5),
        [255, 255, 255, 255],
        "the margin is white"
    );
}

#[test]
fn criterion_13_sizes_outside_64_to_4096_are_errors() {
    for size in [0, 63, 4097, 100_000] {
        assert_eq!(
            board_png(&image(size), false, &font()),
            Err(ImageError::SizeOutOfRange(size))
        );
    }
    assert!(SIZE_RANGE.contains(&64) && SIZE_RANGE.contains(&4096));
    assert!(board_png(&image(64), false, &font()).is_ok());
    let mut img = image(256);
    img.margin_px = 5000;
    assert_eq!(
        board_png(&img, false, &font()),
        Err(ImageError::MarginTooLarge(5000))
    );
}

#[test]
fn criterion_13_a_missing_piece_is_a_clear_error() {
    let missing = PieceSvgs::new(|_, _| String::new());
    assert!(matches!(missing, Err(ImageError::BadPiece { piece: "wK" })));
    let no_black_queen = PieceSvgs::new(|color, role| {
        if color == mychess_core::Color::Black && role == mychess_core::Role::Queen {
            "<svg>no viewBox</svg>".to_owned()
        } else {
            r#"<svg viewBox="0 0 45 45"><rect width="45" height="45"/></svg>"#.to_owned()
        }
    });
    assert!(matches!(
        no_black_queen,
        Err(ImageError::BadPiece { piece: "bQ" })
    ));
    assert_eq!(
        ImageError::BadPiece { piece: "bQ" }.message_key(),
        "export-err-image-piece"
    );
}

#[test]
fn criterion_13_an_invalid_font_is_an_error_not_a_panic() {
    for bad in [&b""[..], b"not a font at all", &[0u8; 64]] {
        assert_eq!(
            board_png(&image(256), false, bad),
            Err(ImageError::InvalidFont)
        );
    }
    // Without coordinates no font is needed.
    let mut img = image(256);
    img.coordinates = false;
    assert!(board_png(&img, false, b"").is_ok());
}

#[test]
fn rule_27_the_coordinates_are_rasterized_with_the_embedded_font() {
    let mut with = image(512);
    with.position = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let mut without = with.clone();
    without.coordinates = false;
    let (_, _, a) = decode(&board_png(&with, false, &font()).unwrap());
    let (_, _, b) = decode(&board_png(&without, false, &font()).unwrap());
    let differing = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
    assert!(differing > 100, "labels paint {differing} pixels");
}
