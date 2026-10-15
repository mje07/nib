//! Split tree for a tab page.

use crate::types::{Rect, WinId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// Windows stacked top to bottom (`:split`).
    Horizontal,
    /// Windows side by side (`:vsplit`).
    Vertical,
}

#[derive(Clone, Debug)]
pub enum Node {
    Leaf(WinId),
    Split { dir: Dir, children: Vec<(Node, f64)> },
}

pub fn contains(n: &Node, id: WinId) -> bool {
    match n {
        Node::Leaf(w) => *w == id,
        Node::Split { children, .. } => children.iter().any(|(c, _)| contains(c, id)),
    }
}

pub fn leaves(n: &Node) -> Vec<WinId> {
    match n {
        Node::Leaf(w) => vec![*w],
        Node::Split { children, .. } => children.iter().flat_map(|(c, _)| leaves(c)).collect(),
    }
}

/// Split window `target`, placing `new` before (above/left) or after it.
pub fn split(n: &mut Node, target: WinId, new: WinId, dir: Dir, after: bool) -> bool {
    match n {
        Node::Leaf(w) if *w == target => {
            let (a, b) = if after { (target, new) } else { (new, target) };
            *n = Node::Split { dir, children: vec![(Node::Leaf(a), 1.0), (Node::Leaf(b), 1.0)] };
            true
        }
        Node::Leaf(_) => false,
        Node::Split { dir: d, children } => {
            if *d == dir
                && let Some(i) = children.iter().position(|(c, _)| matches!(c, Node::Leaf(w) if *w == target))
            {
                let half = children[i].1 / 2.0;
                children[i].1 = half;
                let idx = if after { i + 1 } else { i };
                children.insert(idx, (Node::Leaf(new), half));
                return true;
            }
            children.iter_mut().any(|(c, _)| split(c, target, new, dir, after))
        }
    }
}

/// Remove a window from the tree. Returns false if it was the only leaf.
pub fn remove(n: &mut Node, id: WinId) -> bool {
    if let Node::Split { children, .. } = n {
        if let Some(i) = children.iter().position(|(c, _)| matches!(c, Node::Leaf(w) if *w == id)) {
            children.remove(i);
        } else {
            for (c, _) in children.iter_mut() {
                if remove(c, id) {
                    break;
                }
            }
        }
        if children.len() == 1 {
            let (only, _) = children.remove(0);
            *n = only;
        }
        return true;
    }
    false
}

pub fn equalize(n: &mut Node) {
    if let Node::Split { children, .. } = n {
        for (c, w) in children.iter_mut() {
            *w = 1.0;
            equalize(c);
        }
    }
}

fn distribute(total: u16, weights: &[f64], gaps: u16) -> Vec<u16> {
    let avail = total.saturating_sub(gaps) as f64;
    let sum: f64 = weights.iter().sum::<f64>().max(f64::EPSILON);
    let mut sizes: Vec<u16> = weights.iter().map(|w| ((w / sum) * avail).floor().max(1.0) as u16).collect();
    let used: u16 = sizes.iter().sum();
    let target = total.saturating_sub(gaps);
    if let Some(last) = sizes.last_mut() {
        if used < target {
            *last += target - used;
        } else if used > target {
            *last = last.saturating_sub(used - target).max(1);
        }
    }
    sizes
}

/// Rects for each window. Vertical splits get a 1-column separator.
pub fn rects(n: &Node, area: Rect) -> Vec<(WinId, Rect)> {
    let mut out = Vec::new();
    layout(n, area, &mut out);
    out
}

fn layout(n: &Node, area: Rect, out: &mut Vec<(WinId, Rect)>) {
    match n {
        Node::Leaf(w) => out.push((*w, area)),
        Node::Split { dir, children } => {
            let weights: Vec<f64> = children.iter().map(|(_, w)| *w).collect();
            match dir {
                Dir::Horizontal => {
                    let sizes = distribute(area.h, &weights, 0);
                    let mut y = area.y;
                    for ((c, _), h) in children.iter().zip(sizes) {
                        layout(c, Rect::new(area.x, y, area.w, h), out);
                        y += h;
                    }
                }
                Dir::Vertical => {
                    let gaps = children.len().saturating_sub(1) as u16;
                    let sizes = distribute(area.w, &weights, gaps);
                    let mut x = area.x;
                    for ((c, _), w) in children.iter().zip(sizes) {
                        layout(c, Rect::new(x, area.y, w, area.h), out);
                        x += w + 1;
                    }
                }
            }
        }
    }
}

/// Resize window `id` by `delta` cells along `dir`.
pub fn resize(n: &mut Node, area: Rect, id: WinId, dir: Dir, delta: i32) -> bool {
    resize_inner(n, area, id, dir, delta).is_some()
}

fn resize_inner(n: &mut Node, area: Rect, id: WinId, dir: Dir, delta: i32) -> Option<()> {
    let Node::Split { dir: d, children } = n else { return None };
    let gaps = if *d == Dir::Vertical { children.len().saturating_sub(1) as u16 } else { 0 };
    let total = if *d == Dir::Vertical { area.w } else { area.h };
    let weights: Vec<f64> = children.iter().map(|(_, w)| *w).collect();
    let sizes = distribute(total, &weights, gaps);
    let idx = children.iter().position(|(c, _)| contains(c, id))?;
    // Recurse first: the innermost matching split wins.
    let mut off = 0u16;
    for s in sizes.iter().take(idx) {
        off += s + if *d == Dir::Vertical { 1 } else { 0 };
    }
    let sub = if *d == Dir::Vertical {
        Rect::new(area.x + off, area.y, sizes[idx], area.h)
    } else {
        Rect::new(area.x, area.y + off, area.w, sizes[idx])
    };
    if resize_inner(&mut children[idx].0, sub, id, dir, delta).is_some() {
        return Some(());
    }
    if *d != dir || children.len() < 2 {
        return None;
    }
    let other = if idx + 1 < children.len() { idx + 1 } else { idx - 1 };
    let mut sz: Vec<i32> = sizes.iter().map(|&s| s as i32).collect();
    let d2 = delta.clamp(1 - sz[idx], sz[other] - 1);
    sz[idx] += d2;
    sz[other] -= d2;
    for (i, (_, w)) in children.iter_mut().enumerate() {
        *w = sz[i].max(1) as f64;
    }
    Some(())
}

/// Maximise `id` within its split along `dir`.
pub fn maximize(n: &mut Node, id: WinId, dir: Dir) {
    if let Node::Split { dir: d, children } = n {
        let idx = children.iter().position(|(c, _)| contains(c, id));
        if let Some(idx) = idx {
            if *d == dir {
                for (i, (_, w)) in children.iter_mut().enumerate() {
                    *w = if i == idx { 1000.0 } else { 1.0 };
                }
            }
            maximize(&mut children[idx].0, id, dir);
        }
    }
}
