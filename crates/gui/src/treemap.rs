use clrs_core::{EntryKind, NodeId, Safety, Tree};

use crate::category::{self, Category};
use crate::recommendations::Suggestions;

/// Most tiles drawn for one folder, including the tile for smaller entries.
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

/// Smallest share of the folder drawn as its own tile; the rest join the tail tile.
const MIN_SHARE: f64 = 0.0015;
/// Children drawn inside a tile large enough to show them.
const MAX_NESTED: usize = 12;
const NESTED_MIN: f32 = 90.;
const NESTED_TOP: f32 = 44.;
const NESTED_INSET: f32 = 6.;

/// One drawn tile, ready to render.
#[derive(Debug)]
pub(crate) struct Block {
    /// `None` for the tile that stands for every entry too small to draw.
    pub(crate) id: Option<NodeId>,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) category: Category,
    pub(crate) safety: Option<Safety>,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) nested: Vec<Nested>,
}

#[derive(Debug)]
pub(crate) struct Nested {
    pub(crate) id: NodeId,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Lays out `folder`'s children as tiles in a `width` × `height` area, folding entries
/// smaller than [`MIN_SHARE`] (or beyond [`MAX_TILES`]) into one "smaller items" tile.
pub(crate) fn blocks(
    tree: &Tree,
    folder: NodeId,
    suggestions: &Suggestions,
    width: f32,
    height: f32,
) -> Vec<Block> {
    let total = tree.node(folder).size();
    let children: Vec<NodeId> = tree.children(folder).collect();
    let drawn = children
        .iter()
        .take(MAX_TILES - 1)
        .take_while(|&&id| crate::ui::share(tree.node(id).size(), total) >= MIN_SHARE)
        .count();
    let rest = &children[drawn..];
    let tail: u64 = rest.iter().map(|&id| tree.node(id).size()).sum();
    let mut entries: Vec<(Option<NodeId>, u64)> = children[..drawn]
        .iter()
        .map(|&id| (Some(id), tree.node(id).size()))
        .collect();
    if tail > 0 {
        let at = entries.partition_point(|&(_, size)| size >= tail);
        entries.insert(at, (None, tail));
    }
    let sizes: Vec<u64> = entries.iter().map(|&(_, size)| size).collect();
    layout(&sizes, width, height)
        .into_iter()
        .map(|tile| {
            let (id, size) = entries[tile.row];
            let Some(id) = id else {
                return Block {
                    id: None,
                    name: format!("{} smaller items", crate::format::count(rest.len() as u64)),
                    size,
                    category: Category::Other,
                    safety: None,
                    x: tile.x,
                    y: tile.y,
                    width: tile.width,
                    height: tile.height,
                    nested: Vec::new(),
                };
            };
            let nested = if tile.width >= NESTED_MIN
                && tile.height >= NESTED_MIN
                && tree.node(id).kind() == EntryKind::Dir
            {
                let inner: Vec<NodeId> = tree.children(id).take(MAX_NESTED).collect();
                let sizes: Vec<u64> = inner.iter().map(|&child| tree.node(child).size()).collect();
                layout(
                    &sizes,
                    tile.width - 2. * NESTED_INSET,
                    tile.height - NESTED_TOP - NESTED_INSET,
                )
                .into_iter()
                .map(|inner_tile| {
                    let child = inner[inner_tile.row];
                    Nested {
                        id: child,
                        name: tree.name(child).to_string_lossy().into_owned(),
                        size: tree.node(child).size(),
                        x: inner_tile.x + NESTED_INSET,
                        y: inner_tile.y + NESTED_TOP,
                        width: inner_tile.width,
                        height: inner_tile.height,
                    }
                })
                .collect()
            } else {
                Vec::new()
            };
            Block {
                id: Some(id),
                name: tree.name(id).to_string_lossy().into_owned(),
                size,
                category: category::classify(tree, id, suggestions),
                safety: suggestions.of(id).map(|group| group.safety),
                x: tile.x,
                y: tile.y,
                width: tile.width,
                height: tile.height,
                nested,
            }
        })
        .collect()
}
