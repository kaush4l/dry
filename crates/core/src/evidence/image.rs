//! Screenshots as pixels: PNG in and out, flat masks, and the pixel diff
//! (how many changed, where, and an image that shows it).
//!
//! ```text
//! baseline.png ─┐                ┌─▶ changed % (against the threshold)
//!               ├─ diff(tol) ────┼─▶ boxes around the changed regions
//! capture.png ──┘                └─▶ diff.png: the baseline faded, changed
//!                                     pixels red, each region boxed
//! ```
//!
//! Pure (the `png` crate is pure Rust), so it runs on every target, the
//! browser's wasm included. Every image is 8-bit RGBA once decoded; the
//! encoder's settings are fixed, so the same pixels always give the same
//! file (and the same sha256).

use std::io::Cursor;

/// An 8-bit RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// A rectangle in pixels: left, top, width, height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    /// As the summary writes it: `x,y wxh`.
    pub fn text(self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.w, self.h)
    }
}

/// The colour masks are painted with: a flat mid grey, opaque.
pub const MASK: [u8; 4] = [128, 128, 128, 255];
/// A changed pixel in the diff image.
const RED: [u8; 4] = [255, 0, 64, 255];
/// The box drawn around a changed region.
const BOX: [u8; 4] = [255, 0, 255, 255];
/// The changed regions are found on a grid of cells this many pixels wide.
pub const CELL: u32 = 8;
/// The most boxes listed (the rest merge into the last).
pub const MAX_BOXES: usize = 20;

impl Image {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Image {
        let n = width as usize * height as usize;
        Image { width, height, rgba: fill.iter().copied().cycle().take(n * 4).collect() }
    }

    fn at(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = self.at(x, y);
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    pub fn set(&mut self, x: u32, y: u32, c: [u8; 4]) {
        if x < self.width && y < self.height {
            let i = self.at(x, y);
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }

    /// Paint `r` (cut to the image) one flat colour: a mask.
    pub fn fill(&mut self, r: Rect, c: [u8; 4]) {
        for y in r.y..r.y.saturating_add(r.h).min(self.height) {
            for x in r.x..r.x.saturating_add(r.w).min(self.width) {
                self.set(x, y, c);
            }
        }
    }

    /// A box's outline, `t` pixels thick, inside `r`.
    fn outline(&mut self, r: Rect, t: u32, c: [u8; 4]) {
        let (x1, y1) = (r.x + r.w.saturating_sub(1), r.y + r.h.saturating_sub(1));
        for k in 0..t {
            for x in r.x..=x1 {
                self.set(x, r.y + k, c);
                self.set(x, y1.saturating_sub(k), c);
            }
            for y in r.y..=y1 {
                self.set(r.x + k, y, c);
                self.set(x1.saturating_sub(k), y, c);
            }
        }
    }
}

/// Read a PNG into RGBA (any colour type, 8 or 16 bit).
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let mut d = png::Decoder::new(Cursor::new(bytes));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = d.read_info().map_err(|e| format!("not a PNG that reads: {e}"))?;
    let size = reader.output_buffer_size().ok_or("the PNG is too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("the PNG does not decode: {e}"))?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return Err("an indexed PNG was not expanded".into()),
    };
    if rgba.len() != w as usize * h as usize * 4 {
        return Err(format!("the PNG says {w}x{h} but holds {} bytes", rgba.len()));
    }
    Ok(Image { width: w, height: h, rgba })
}

/// Write RGBA as a PNG, always with the same settings: the same pixels
/// give the same bytes.
pub fn encode(img: &Image) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, img.width, img.height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        // Balanced: half the bytes of Fast for about 50 ms a 1280x800 shot.
        e.set_compression(png::Compression::Balanced);
        e.set_filter(png::Filter::Adaptive);
        // Writing to a Vec cannot fail, and the size is the image's own.
        if let Ok(mut w) = e.write_header() {
            let _ = w.write_image_data(&img.rgba);
        }
    }
    out
}

/// What changed between two images.
#[derive(Clone, Debug, PartialEq)]
pub struct Diff {
    /// Pixels that differ by more than the tolerance in some channel.
    pub changed: u64,
    pub total: u64,
    /// The changed regions, boxed (on the `CELL` grid), largest first.
    pub boxes: Vec<Rect>,
    /// The baseline faded, changed pixels red, regions boxed.
    pub image: Image,
    /// The sizes differ (every pixel counts as changed).
    pub resized: bool,
}

impl Diff {
    /// Changed pixels as a percent of all, to two places.
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.changed as f64 * 10_000.0 / self.total as f64).round() / 100.0
    }
}

/// Compare `after` with `before`: a pixel changed when a channel differs
/// by more than `tolerance` (0 to 255).
pub fn diff(before: &Image, after: &Image, tolerance: u8) -> Diff {
    if (before.width, before.height) != (after.width, after.height) {
        let mut image = faded(after);
        let all = Rect::new(0, 0, after.width, after.height);
        image.outline(all, 3, BOX);
        let total = u64::from(after.width) * u64::from(after.height);
        return Diff { changed: total, total, boxes: vec![all], image, resized: true };
    }
    let (w, h) = (after.width, after.height);
    let mut image = faded(before);
    let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut cells = vec![false; gw as usize * gh as usize];
    let mut changed = 0u64;
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (before.pixel(x, y), after.pixel(x, y));
            if a.iter().zip(b).any(|(p, q)| p.abs_diff(q) > tolerance) {
                changed += 1;
                image.set(x, y, RED);
                cells[((y / CELL) * gw + x / CELL) as usize] = true;
            }
        }
    }
    let boxes = regions(&cells, gw, gh, w, h);
    for b in &boxes {
        image.outline(grow(*b, 2, w, h), 2, BOX);
    }
    Diff { changed, total: u64::from(w) * u64::from(h), boxes, image, resized: false }
}

/// The image grey and light, so red marks stand out on it.
fn faded(img: &Image) -> Image {
    let mut out = img.clone();
    for p in out.rgba.as_chunks_mut::<4>().0 {
        let g = ((u32::from(p[0]) * 30 + u32::from(p[1]) * 59 + u32::from(p[2]) * 11) / 100) as u8;
        let v = 160 + g / 3;
        p.copy_from_slice(&[v, v, v, 255]);
    }
    out
}

fn grow(r: Rect, by: u32, w: u32, h: u32) -> Rect {
    let x = r.x.saturating_sub(by);
    let y = r.y.saturating_sub(by);
    Rect::new(x, y, (r.x + r.w + by).min(w) - x, (r.y + r.h + by).min(h) - y)
}

/// Group the changed cells that touch (8 ways) into boxes, in pixels.
fn regions(cells: &[bool], gw: u32, gh: u32, w: u32, h: u32) -> Vec<Rect> {
    let mut seen = vec![false; cells.len()];
    let mut boxes: Vec<(u64, Rect)> = Vec::new();
    for start in 0..cells.len() {
        if !cells[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        let mut n = 0u64;
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (cx, cy) = (i as u32 % gw, i as u32 / gw);
            n += 1;
            (x0, y0, x1, y1) = (x0.min(cx), y0.min(cy), x1.max(cx), y1.max(cy));
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (i64::from(cx) + dx, i64::from(cy) + dy);
                    if nx < 0 || ny < 0 || nx >= i64::from(gw) || ny >= i64::from(gh) {
                        continue;
                    }
                    let j = (ny as u32 * gw + nx as u32) as usize;
                    if cells[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let (px, py) = (x0 * CELL, y0 * CELL);
        let r = Rect::new(px, py, ((x1 + 1) * CELL).min(w) - px, ((y1 + 1) * CELL).min(h) - py);
        boxes.push((n, r));
    }
    boxes.sort_by(|a, b| b.0.cmp(&a.0).then((a.1.y, a.1.x).cmp(&(b.1.y, b.1.x))));
    let mut out: Vec<Rect> = boxes.into_iter().map(|(_, r)| r).collect();
    if out.len() > MAX_BOXES {
        let rest = out.split_off(MAX_BOXES - 1);
        let (x0, y0) = (rest.iter().map(|r| r.x).min().unwrap_or(0), rest.iter().map(|r| r.y).min().unwrap_or(0));
        let (x1, y1) = (rest.iter().map(|r| r.x + r.w).max().unwrap_or(0), rest.iter().map(|r| r.y + r.h).max().unwrap_or(0));
        out.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    #[test]
    fn a_png_reads_back_as_the_same_pixels_and_the_same_bytes() {
        let mut img = Image::new(37, 11, WHITE);
        img.fill(Rect::new(3, 2, 5, 4), [10, 20, 30, 255]);
        img.set(36, 10, [1, 2, 3, 4]);
        let bytes = encode(&img);
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(decode(&bytes).unwrap(), img);
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes, "the same pixels give the same file");
        assert!(decode(b"not a png").unwrap_err().contains("not a PNG"));
    }

    #[test]
    fn rgb_and_grey_pngs_read_as_rgba() {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 2, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        }
        assert_eq!(decode(&out).unwrap().rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn the_same_image_has_no_change() {
        let img = Image::new(64, 40, WHITE);
        let d = diff(&img, &img, 0);
        assert_eq!((d.changed, d.percent(), d.boxes.len(), d.resized), (0, 0.0, 0, false));
    }

    #[test]
    fn changed_pixels_are_counted_boxed_and_drawn() {
        let before = Image::new(100, 50, WHITE);
        let mut after = before.clone();
        // Two separate regions: a 10x10 block, and a 4x2 one far away.
        after.fill(Rect::new(5, 5, 10, 10), [0, 0, 0, 255]);
        after.fill(Rect::new(80, 40, 4, 2), [0, 0, 0, 255]);
        let d = diff(&before, &after, 8);
        assert_eq!(d.changed, 108);
        assert_eq!(d.total, 5000);
        assert_eq!(d.percent(), 2.16);
        assert_eq!(d.boxes, vec![Rect::new(0, 0, 16, 16), Rect::new(80, 40, 8, 8)], "largest first, on the 8-pixel grid");
        assert_eq!(d.image.pixel(10, 10), RED, "a changed pixel is red");
        assert_eq!(d.image.pixel(50, 25)[0], d.image.pixel(50, 25)[1], "the rest is faded grey");
        assert_eq!(d.image.pixel(0, 0), BOX, "the box is drawn around the region");
    }

    #[test]
    fn small_changes_within_the_tolerance_do_not_count() {
        let before = Image::new(10, 10, [100, 100, 100, 255]);
        let after = Image::new(10, 10, [104, 97, 100, 255]);
        assert_eq!(diff(&before, &after, 4).changed, 0);
        assert_eq!(diff(&before, &after, 3).changed, 100);
    }

    #[test]
    fn a_resized_image_is_all_changed() {
        let d = diff(&Image::new(10, 10, WHITE), &Image::new(10, 12, WHITE), 0);
        assert!(d.resized);
        assert_eq!((d.percent(), d.boxes.len()), (100.0, 1));
    }

    #[test]
    fn a_mask_paints_flat_and_stays_inside() {
        let mut img = Image::new(10, 10, WHITE);
        img.fill(Rect::new(8, 8, 5, 5), MASK);
        assert_eq!((img.pixel(9, 9), img.pixel(7, 7)), (MASK, WHITE));
    }

    #[test]
    fn many_regions_are_capped() {
        let before = Image::new(400, 400, WHITE);
        let mut after = before.clone();
        for i in 0..30 {
            after.set((i % 10) * 40, (i / 10) * 40, [0, 0, 0, 255]);
        }
        assert_eq!(diff(&before, &after, 0).boxes.len(), MAX_BOXES);
    }
}
