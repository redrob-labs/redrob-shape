//! Compositor node operations on an 8-bit RGBA frame (`shape_scene::CompositorTree`).

use shape_scene::{CompositorNode, CompositorTree};

/// Run the chain over `rgba` (top row first, `width * height * 4` bytes). Disabled trees return
/// the input unchanged.
pub fn apply(tree: &CompositorTree, width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    if !tree.enabled || tree.nodes.is_empty() || rgba.len() != (width * height * 4) as usize {
        return rgba.to_vec();
    }
    let mut img: Vec<[f32; 3]> = rgba.as_chunks::<4>().0.iter().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
    for node in &tree.nodes {
        img = apply_node(node, width as usize, height as usize, &img);
    }
    let mut out = Vec::with_capacity(rgba.len());
    for (p, src) in img.iter().zip(rgba.as_chunks::<4>().0.iter()) {
        out.extend_from_slice(&[to8(p[0]), to8(p[1]), to8(p[2]), src[3]]);
    }
    out
}

fn to8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn apply_node(node: &CompositorNode, w: usize, h: usize, img: &[[f32; 3]]) -> Vec<[f32; 3]> {
    match node {
        CompositorNode::Blur { size } => blur(w, h, img, *size as usize),
        CompositorNode::BrightContrast { bright, contrast } => {
            // Blender's formula: a = 1 + contrast/100, b = bright/100 - contrast/200 (around 0.5).
            let a = 1.0 + contrast / 100.0;
            let b = bright / 100.0 - contrast / 200.0;
            img.iter().map(|p| p.map(|c| a * c + b)).collect()
        }
        CompositorNode::Glare { threshold, mix, size } => {
            let bright: Vec<[f32; 3]> = img.iter().map(|p| if luminance(p) > *threshold { *p } else { [0.0; 3] }).collect();
            let halo = blur(w, h, &bright, *size as usize);
            img.iter().zip(&halo).map(|(p, g)| [p[0] + g[0] * mix, p[1] + g[1] * mix, p[2] + g[2] * mix]).collect()
        }
        CompositorNode::Gamma { gamma } => {
            let g = gamma.max(0.01);
            img.iter().map(|p| p.map(|c| c.max(0.0).powf(1.0 / g))).collect()
        }
        CompositorNode::HueSaturation { hue, saturation, value } => img
            .iter()
            .map(|p| {
                let (mut hh, s, v) = rgb_to_hsv(*p);
                hh = (hh + (hue - 0.5)).rem_euclid(1.0);
                hsv_to_rgb(hh, (s * saturation).clamp(0.0, 1.0), v * value)
            })
            .collect(),
        CompositorNode::MixColor { color, factor } => {
            let f = factor.clamp(0.0, 1.0);
            img.iter().map(|p| [p[0] + (color[0] - p[0]) * f, p[1] + (color[1] - p[1]) * f, p[2] + (color[2] - p[2]) * f]).collect()
        }
        CompositorNode::Invert => img.iter().map(|p| p.map(|c| 1.0 - c)).collect(),
    }
}

fn luminance(p: &[f32; 3]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

/// Separable box blur run three times (close to Gaussian), radius `size`.
fn blur(w: usize, h: usize, img: &[[f32; 3]], size: usize) -> Vec<[f32; 3]> {
    if size == 0 || w == 0 || h == 0 {
        return img.to_vec();
    }
    let r = size.max(1);
    let mut cur = img.to_vec();
    for _ in 0..3 {
        let mut tmp = vec![[0.0f32; 3]; cur.len()];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for dx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    let p = cur[y * w + dx];
                    acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
                    n += 1.0;
                }
                tmp[y * w + x] = acc.map(|c| c / n);
            }
        }
        for x in 0..w {
            for y in 0..h {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for dy in y.saturating_sub(r)..=(y + r).min(h - 1) {
                    let p = tmp[dy * w + x];
                    acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
                    n += 1.0;
                }
                cur[y * w + x] = acc.map(|c| c / n);
            }
        }
    }
    cur
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if max > 0.0 { d / max } else { 0.0 }, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match (i as i32).rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_tree_is_identity_and_nodes_change_pixels() {
        let (w, h) = (4u32, 2u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i * 30) as u8, 100, 200, 255]).collect();
        let off = CompositorTree { enabled: false, nodes: vec![CompositorNode::Invert] };
        assert_eq!(apply(&off, w, h, &rgba), rgba);
        let inv = CompositorTree { enabled: true, nodes: vec![CompositorNode::Invert] };
        let out = apply(&inv, w, h, &rgba);
        assert_eq!(out[1], 155);
        assert_eq!(out[3], 255, "alpha kept");
        let blur = CompositorTree { enabled: true, nodes: vec![CompositorNode::Blur { size: 2 }] };
        let out = apply(&blur, w, h, &rgba);
        assert!(out[0] > rgba[0], "blur pulls the dark corner up");
        let bc = CompositorTree { enabled: true, nodes: vec![CompositorNode::BrightContrast { bright: 50.0, contrast: 0.0 }] };
        assert_eq!(apply(&bc, w, h, &rgba)[1], 228);
        let hsv = CompositorTree { enabled: true, nodes: vec![CompositorNode::HueSaturation { hue: 0.5, saturation: 1.0, value: 1.0 }] };
        let same = apply(&hsv, w, h, &rgba);
        assert!(same.iter().zip(&rgba).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 1), "neutral HSV keeps the image");
    }
}
