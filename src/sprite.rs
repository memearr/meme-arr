//! The built-in shapes an export can draw over a clip (an arrow, a ring, a box, a
//! dot): rasterised here to a transparent PNG that ffmpeg then overlays like any
//! other picture. Drawing them ourselves keeps the export free of an SVG renderer
//! and makes the picture the editor previews the exact one the export uses.

/// Sprites are square, this many pixels a side: big enough to scale down cleanly to
/// any size a clip needs, small enough to be a quick file.
pub const SPRITE_SIZE: u32 = 512;

/// Which shapes there are, as the API names them.
pub const SHAPES: &[(&str, &str)] = &[
    ("arrow", "Arrow (up)"),
    ("ring", "Ring"),
    ("box", "Box"),
    ("dot", "Dot"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// The chunky upvote arrow, pointing up (turn it with the layer's rotation).
    Arrow,
    /// A circle outline.
    Ring,
    /// A rectangle outline.
    Box,
    /// A filled circle.
    Dot,
}

impl Shape {
    pub fn parse(name: &str) -> Option<Shape> {
        match name {
            "arrow" => Some(Shape::Arrow),
            "ring" => Some(Shape::Ring),
            "box" => Some(Shape::Box),
            "dot" => Some(Shape::Dot),
            _ => None,
        }
    }
}

/// The arrow's outline in the unit square (y grows downward).
const ARROW: [(f32, f32); 7] = [
    (0.5, 0.02),
    (0.98, 0.52),
    (0.72, 0.52),
    (0.72, 0.98),
    (0.28, 0.98),
    (0.28, 0.52),
    (0.02, 0.52),
];

fn inside_polygon(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether the point (x, y) in the unit square is part of the shape.
fn covered(shape: Shape, x: f32, y: f32) -> bool {
    match shape {
        Shape::Arrow => inside_polygon(&ARROW, x, y),
        Shape::Dot => (x - 0.5).hypot(y - 0.5) <= 0.48,
        Shape::Ring => {
            let d = (x - 0.5).hypot(y - 0.5);
            (0.36..=0.48).contains(&d)
        }
        Shape::Box => {
            let inner = (0.12..=0.88).contains(&x) && (0.12..=0.88).contains(&y);
            let outer = (0.02..=0.98).contains(&x) && (0.02..=0.98).contains(&y);
            outer && !inner
        }
    }
}

/// The shape in `rgb` as `SPRITE_SIZE` x `SPRITE_SIZE` RGBA pixels, with soft edges
/// (each pixel is sampled 4 x 4 times for its coverage).
pub fn render_rgba(shape: Shape, rgb: [u8; 3]) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let n = SPRITE_SIZE;
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for py in 0..n {
        for px in 0..n {
            let mut hit = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / n as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / n as f32;
                    if covered(shape, x, y) {
                        hit += 1;
                    }
                }
            }
            let alpha = (hit * 255 / (SAMPLES * SAMPLES)) as u8;
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A PNG of `width` x `height` RGBA pixels. The pixel data is stored, not
/// compressed: these are short-lived temp files read once by ffmpeg, and storing keeps
/// this to a few lines with no compression library.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), (width * height * 4) as usize);
    // each row is a filter byte (0 = none) and then its pixels
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks((width * 4) as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib: header, stored deflate blocks of at most 65535 bytes, Adler-32
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// The shape as a PNG file's bytes.
pub fn shape_png(shape: Shape, rgb: [u8; 3]) -> Vec<u8> {
    encode_png(SPRITE_SIZE, SPRITE_SIZE, &render_rgba(shape, rgb))
}

/// Reddit's upvote orange: what a shape is drawn in unless told otherwise.
pub const DEFAULT_COLOR: [u8; 3] = [255, 69, 0];

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(px: &[u8], x: f32, y: f32) -> u8 {
        let n = SPRITE_SIZE as f32;
        let (cx, cy) = ((x * n) as usize, (y * n) as usize);
        px[(cy * SPRITE_SIZE as usize + cx) * 4 + 3]
    }

    #[test]
    fn checksums_match_the_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_png_has_its_signature_size_and_end() {
        let png = shape_png(Shape::Dot, [1, 2, 3]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(
            u32::from_be_bytes(png[16..20].try_into().unwrap()),
            SPRITE_SIZE
        );
        assert_eq!(
            u32::from_be_bytes(png[20..24].try_into().unwrap()),
            SPRITE_SIZE
        );
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        // a block larger than 65535 bytes was split: more than one IDAT-internal block
        assert!(png.len() > (SPRITE_SIZE * SPRITE_SIZE * 4) as usize);
    }

    #[test]
    fn a_tiny_png_is_exactly_what_the_format_says() {
        // 1x1 opaque red, checked against the PNG spec by hand: one row of
        // filter byte 0 + 4 pixel bytes in a single final stored block
        let png = encode_png(1, 1, &[255, 0, 0, 255]);
        let idat = png.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(png[idat - 4..idat].try_into().unwrap()) as usize;
        let data = &png[idat + 4..idat + 4 + len];
        assert_eq!(&data[..2], &[0x78, 0x01]); // zlib header
        assert_eq!(data[2], 1); // final stored block
        assert_eq!(&data[3..7], &[5, 0, 0xFA, 0xFF]); // LEN = 5, NLEN = !5
        assert_eq!(&data[7..12], &[0, 255, 0, 0, 255]);
    }

    #[test]
    fn the_arrow_has_a_head_a_shaft_and_clear_corners() {
        let px = render_rgba(Shape::Arrow, DEFAULT_COLOR);
        assert_eq!(alpha_at(&px, 0.5, 0.2), 255, "head");
        assert_eq!(alpha_at(&px, 0.5, 0.8), 255, "shaft");
        assert_eq!(alpha_at(&px, 0.1, 0.9), 0, "beside the shaft");
        assert_eq!(alpha_at(&px, 0.05, 0.05), 0, "corner");
        // colour is kept at every pixel
        assert_eq!(&px[(256 * 512 + 256) * 4..][..3], &DEFAULT_COLOR);
    }

    #[test]
    fn rings_and_boxes_are_hollow_and_dots_are_solid() {
        let ring = render_rgba(Shape::Ring, [0, 0, 0]);
        assert_eq!(alpha_at(&ring, 0.5, 0.5), 0);
        assert_eq!(alpha_at(&ring, 0.5, 0.07), 255);
        assert_eq!(alpha_at(&ring, 0.02, 0.02), 0);
        let b = render_rgba(Shape::Box, [0, 0, 0]);
        assert_eq!(alpha_at(&b, 0.5, 0.5), 0);
        assert_eq!(alpha_at(&b, 0.05, 0.5), 255);
        assert_eq!(alpha_at(&b, 0.5, 0.95), 255);
        let dot = render_rgba(Shape::Dot, [0, 0, 0]);
        assert_eq!(alpha_at(&dot, 0.5, 0.5), 255);
        assert_eq!(alpha_at(&dot, 0.03, 0.03), 0);
    }

    #[test]
    fn edges_are_soft() {
        let px = render_rgba(Shape::Dot, [0, 0, 0]);
        let partial = px.chunks(4).filter(|p| p[3] > 0 && p[3] < 255).count();
        assert!(partial > 100, "{partial} partly covered pixels");
    }

    #[test]
    fn shape_names_round_trip() {
        for (name, _) in SHAPES {
            assert!(Shape::parse(name).is_some(), "{name}");
        }
        assert!(Shape::parse("triangle").is_none());
    }
}
