//! DaisyDisk-style ring chart: the open folder in the centre, each ring one level deeper.

use std::f32::consts::{FRAC_PI_2, TAU};

use clrs_core::{EntryKind, NodeId, Tree};
use gpui_kit::{Path, PathBuilder, Pixels, Point, point, px};

use crate::category::{self, Category};
use crate::recommendations::Suggestions;

/// Rings drawn around the centre.
pub(crate) const DEPTH: u8 = 5;
/// Narrowest wedge drawn on its own, in radians; narrower siblings join one grey wedge.
const MIN_SWEEP: f32 = 0.012;
/// Most wedges laid out for one folder.
const MAX_SEGMENTS: usize = 4000;
/// The centre disc's share of the chart radius.
const CENTER_SHARE: f32 = 0.30;
/// Gap between rings and between neighbouring wedges, in pixels.
const GAP: f32 = 1.5;

#[derive(Debug)]
pub(crate) struct Segment {
    /// `None` for the wedge standing for every sibling too small to draw.
    pub(crate) id: Option<NodeId>,
    /// 1 for the folder's children, 2 for grandchildren, and so on.
    pub(crate) depth: u8,
    /// Clockwise from 12 o'clock, in radians.
    pub(crate) start: f32,
    pub(crate) sweep: f32,
    pub(crate) size: u64,
    /// How many entries the grey wedge stands for.
    pub(crate) folded: usize,
    pub(crate) category: Category,
    /// The wedge one ring further in that this one sits inside.
    pub(crate) parent: Option<usize>,
}

impl Segment {
    fn contains(&self, angle: f32) -> bool {
        angle >= self.start && angle < self.start + self.sweep
    }
}

/// Lays out `folder`'s descendants, [`DEPTH`] levels deep. Wedge angles are proportional
/// to size, so the layout doesn't depend on the drawn size and is reused on resize.
pub(crate) fn segments(tree: &Tree, folder: NodeId, suggestions: &Suggestions) -> Vec<Segment> {
    let mut out = Vec::new();
    walk(tree, folder, 0., TAU, 1, None, suggestions, &mut out);
    out
}

#[expect(clippy::too_many_arguments, reason = "recursive layout state")]
#[expect(clippy::cast_precision_loss, reason = "sizes only set angles")]
fn walk(
    tree: &Tree,
    id: NodeId,
    start: f32,
    sweep: f32,
    depth: u8,
    parent: Option<usize>,
    suggestions: &Suggestions,
    out: &mut Vec<Segment>,
) {
    let total = tree.node(id).size();
    if total == 0 {
        return;
    }
    let scale = f64::from(sweep) / total as f64;
    let mut angle = start;
    let mut folded = 0;
    let mut rest = 0;
    for child in tree.children(id) {
        let size = tree.node(child).size();
        if size == 0 {
            break;
        }
        #[expect(clippy::cast_possible_truncation, reason = "an angle")]
        let child_sweep = (size as f64 * scale) as f32;
        if folded > 0 || child_sweep < MIN_SWEEP || out.len() >= MAX_SEGMENTS {
            folded += 1;
            rest += size;
            continue;
        }
        out.push(Segment {
            id: Some(child),
            depth,
            start: angle,
            sweep: child_sweep,
            size,
            folded: 0,
            category: category::classify(tree, child, suggestions),
            parent,
        });
        if depth < DEPTH && tree.node(child).kind() == EntryKind::Dir {
            let index = out.len() - 1;
            walk(
                tree,
                child,
                angle,
                child_sweep,
                depth + 1,
                Some(index),
                suggestions,
                out,
            );
        }
        angle += child_sweep;
    }
    #[expect(clippy::cast_possible_truncation, reason = "an angle")]
    let rest_sweep = (rest as f64 * scale) as f32;
    if rest > 0 && rest_sweep >= MIN_SWEEP / 2. {
        out.push(Segment {
            id: None,
            depth,
            start: angle,
            sweep: rest_sweep,
            size: rest,
            folded,
            category: Category::Other,
            parent,
        });
    }
}

/// Whether `index` is `ancestor` or drawn inside it on an outer ring.
pub(crate) fn within(segments: &[Segment], index: usize, ancestor: usize) -> bool {
    let mut current = Some(index);
    while let Some(at) = current {
        if at == ancestor {
            return true;
        }
        if at < ancestor {
            return false;
        }
        current = segments[at].parent;
    }
    false
}

/// Where the rings sit inside the drawn area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) center: Point<Pixels>,
    pub(crate) hole: f32,
    pub(crate) ring: f32,
}

/// What lies under a point of the chart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Center,
    Segment(usize),
}

impl Geometry {
    pub(crate) fn new(center: Point<Pixels>, width: f32, height: f32) -> Self {
        let radius = (width.min(height) / 2. - 6.).max(0.);
        let hole = radius * CENTER_SHARE;
        Self {
            center,
            hole,
            ring: (radius - hole) / f32::from(DEPTH),
        }
    }

    pub(crate) fn hit(&self, at: Point<Pixels>, segments: &[Segment]) -> Option<Hit> {
        let dx = f32::from(at.x - self.center.x);
        let dy = f32::from(at.y - self.center.y);
        let distance = dx.hypot(dy);
        if distance < self.hole {
            return Some(Hit::Center);
        }
        if self.ring <= 0. {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small ring index"
        )]
        let depth = ((distance - self.hole) / self.ring) as u8 + 1;
        if depth > DEPTH {
            return None;
        }
        let angle = (dy.atan2(dx) + FRAC_PI_2).rem_euclid(TAU);
        segments
            .iter()
            .position(|segment| segment.depth == depth && segment.contains(angle))
            .map(Hit::Segment)
    }

    /// The centre disc, which stands for the open folder.
    #[expect(clippy::cast_precision_loss, reason = "a small step count")]
    pub(crate) fn disc(&self) -> Option<Path<Pixels>> {
        const STEPS: usize = 96;
        let radius = self.hole - GAP;
        if radius <= 0. {
            return None;
        }
        let mut path = PathBuilder::fill();
        for step in 0..STEPS {
            let angle = TAU * step as f32 / STEPS as f32;
            let at = point(
                self.center.x + px(radius * angle.cos()),
                self.center.y + px(radius * angle.sin()),
            );
            if step == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }
        path.close();
        path.build().ok()
    }

    /// The filled shape of `segment`, with a small gap to its neighbours.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a small step count"
    )]
    pub(crate) fn wedge(&self, segment: &Segment, stroke: Option<Pixels>) -> Option<Path<Pixels>> {
        let inner = self.hole + f32::from(segment.depth - 1) * self.ring + GAP / 2.;
        let outer = inner + self.ring - GAP;
        if outer <= inner {
            return None;
        }
        let full = segment.sweep >= TAU - 0.0001;
        let pad = if full {
            0.
        } else {
            (GAP / 2. / inner).min(segment.sweep / 4.)
        };
        let (start, sweep) = (segment.start + pad, segment.sweep - 2. * pad);
        let steps = ((sweep / 0.03).ceil() as usize).max(1);
        let at = |radius: f32, step: usize| {
            let angle = start + sweep * step as f32 / steps as f32 - FRAC_PI_2;
            point(
                self.center.x + px(radius * angle.cos()),
                self.center.y + px(radius * angle.sin()),
            )
        };
        let mut path = stroke.map_or_else(PathBuilder::fill, PathBuilder::stroke);
        path.move_to(at(outer, 0));
        for step in 1..=steps {
            path.line_to(at(outer, step));
        }
        if full {
            // Two opposite loops, so the fill leaves the hole open.
            path.close();
            path.move_to(at(inner, steps));
        } else {
            path.line_to(at(inner, steps));
        }
        for step in (0..steps).rev() {
            path.line_to(at(inner, step));
        }
        path.close();
        path.build().ok()
    }
}
