use gpui_kit::{Hsla, hsla};

/// Most tiles drawn for one folder; smaller entries are summarised in the caption.
pub(crate) const MAX_TILES: usize = 150;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tile {
    /// Index of the entry in the folder's rows.
    pub(crate) row: usize,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Squarified layout (Bruls, Huizing, van Wijk) of `sizes` into a `width` × `height` area.
///
/// `sizes` must be largest first, which is how the scan tree stores children. Layout stops
/// at the first zero size or after [`MAX_TILES`] entries, and the tiles that are placed fill
/// the whole area.
#[expect(
    clippy::cast_precision_loss,
    reason = "sizes only set pixel proportions"
)]
pub(crate) fn layout(sizes: &[u64], width: f32, height: f32) -> Vec<Tile> {
    let count = sizes
        .iter()
        .take(MAX_TILES)
        .take_while(|&&size| size > 0)
        .count();
    let total: u64 = sizes[..count].iter().sum();
    if count == 0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    let scale = f64::from(width) * f64::from(height) / total as f64;
    let areas: Vec<f64> = sizes[..count].iter().map(|&s| s as f64 * scale).collect();
    let mut tiles = Vec::with_capacity(count);
    let (mut x, mut y) = (0.0, 0.0);
    let (mut w, mut h) = (f64::from(width), f64::from(height));
    let mut start = 0;
    while start < count {
        let side = w.min(h);
        let worst = |row: &[f64], sum: f64| {
            let (largest, smallest) = (row[0], row[row.len() - 1]);
            (side * side * largest / (sum * sum)).max(sum * sum / (side * side * smallest))
        };
        let mut end = start + 1;
        let mut sum = areas[start];
        let mut best = worst(&areas[start..end], sum);
        while end < count {
            let next = sum + areas[end];
            let candidate = worst(&areas[start..=end], next);
            if candidate > best {
                break;
            }
            sum = next;
            best = candidate;
            end += 1;
        }
        if w >= h {
            let strip = sum / h;
            let mut top = y;
            for (offset, &area) in areas[start..end].iter().enumerate() {
                let tile_height = area / strip;
                tiles.push(tile(start + offset, x, top, strip, tile_height));
                top += tile_height;
            }
            x += strip;
            w -= strip;
        } else {
            let strip = sum / w;
            let mut left = x;
            for (offset, &area) in areas[start..end].iter().enumerate() {
                let tile_width = area / strip;
                tiles.push(tile(start + offset, left, y, tile_width, strip));
                left += tile_width;
            }
            y += strip;
            h -= strip;
        }
        start = end;
    }
    tiles
}

#[expect(clippy::cast_possible_truncation)]
fn tile(row: usize, x: f64, y: f64, width: f64, height: f64) -> Tile {
    Tile {
        row,
        x: x as f32,
        y: y as f32,
        width: width as f32,
        height: height as f32,
    }
}

/// Fill colour for the tile at `rank`: hue cycles by rank, folders are more saturated.
pub(crate) fn fill(rank: usize, folder: bool, dark: bool) -> Hsla {
    #[expect(clippy::cast_precision_loss)]
    let hue = (rank * 47 % 360) as f32 / 360.0;
    let saturation = if folder { 0.5 } else { 0.22 };
    hsla(hue, saturation, if dark { 0.3 } else { 0.84 }, 1.0)
}
