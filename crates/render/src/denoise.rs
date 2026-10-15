//! A small edge-aware denoiser for the final frame: a 7x7 bilateral filter whose range weight
//! uses the colour difference, so flat regions average their noise away while edges (where the
//! colour changes a lot) stay put. Cheap and dependency-free; not a replacement for a learned
//! denoiser, but it gives low-sample renders a usable look, which is what Blender's Denoise
//! toggle does for the user.

use rayon::prelude::*;
use shape_math::Vec3;

/// Bilateral filter on linear radiance. The range sigma follows the image's noise level: the
/// median over pixels of the 3x3 standard deviation (edges are a minority, so they do not pull
/// it up), so converged images are left nearly alone, noisy ones are smoothed, and a real edge
/// (a jump far larger than the noise) is kept.
pub fn bilateral(pixels: &[Vec3], width: usize, height: usize) -> Vec<Vec3> {
    if width == 0 || height == 0 || pixels.len() != width * height {
        return pixels.to_vec();
    }
    const R: i32 = 3;
    let sigma_space = 2.0f32;
    let local_std = |x: usize, y: usize| -> f32 {
        let (mut sum, mut sq, mut n) = (Vec3::ZERO, 0.0f32, 0.0f32);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                if xx < 0 || yy < 0 || xx >= width as i32 || yy >= height as i32 {
                    continue;
                }
                let p = pixels[yy as usize * width + xx as usize];
                sum += p;
                sq += p.length_squared();
                n += 1.0;
            }
        }
        let mean = sum / n;
        (sq / n - mean.length_squared()).max(0.0).sqrt()
    };
    let mut stds: Vec<f32> = (0..pixels.len()).map(|i| local_std(i % width, i / width)).collect();
    stds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise = stds[stds.len() / 2];
    let sigma_color = (2.0 * noise).max(1e-3);
    let mut out = vec![Vec3::ZERO; pixels.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let centre = pixels[y * width + x];
            let mut sum = Vec3::ZERO;
            let mut wsum = 0.0;
            for dy in -R..=R {
                let yy = y as i32 + dy;
                if yy < 0 || yy >= height as i32 {
                    continue;
                }
                for dx in -R..=R {
                    let xx = x as i32 + dx;
                    if xx < 0 || xx >= width as i32 {
                        continue;
                    }
                    let p = pixels[yy as usize * width + xx as usize];
                    let d2 = (p - centre).length_squared();
                    let ws = (-((dx * dx + dy * dy) as f32) / (2.0 * sigma_space * sigma_space)).exp();
                    let wc = (-d2 / (2.0 * sigma_color * sigma_color)).exp();
                    let w = ws * wc;
                    sum += p * w;
                    wsum += w;
                }
            }
            *o = if wsum > 0.0 { sum / wsum } else { centre };
        }
    });
    out
}

/// Peak signal-to-noise ratio in dB between two linear images clamped to 0..1.
pub fn psnr(a: &[Vec3], b: &[Vec3]) -> f32 {
    let n = a.len().min(b.len()).max(1) as f32;
    let mse: f32 = a
        .iter()
        .zip(b)
        .map(|(p, q)| {
            let d = p.clamp(Vec3::ZERO, Vec3::ONE) - q.clamp(Vec3::ZERO, Vec3::ONE);
            d.length_squared() / 3.0
        })
        .sum::<f32>()
        / n;
    if mse <= 1e-12 { 99.0 } else { 10.0 * (1.0 / mse).log10() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_on_a_flat_field_is_reduced_and_an_edge_is_kept() {
        let (w, h) = (32usize, 16usize);
        let mut rng = crate::rng::Pcg::new(7, 1);
        let clean: Vec<Vec3> = (0..w * h).map(|i| if i % w < w / 2 { Vec3::splat(0.2) } else { Vec3::splat(0.8) }).collect();
        let noisy: Vec<Vec3> = clean.iter().map(|c| *c + Vec3::splat((rng.next_f32() - 0.5) * 0.1)).collect();
        let out = bilateral(&noisy, w, h);
        assert!(psnr(&out, &clean) > psnr(&noisy, &clean) + 3.0, "denoised {} vs noisy {}", psnr(&out, &clean), psnr(&noisy, &clean));
        // The edge between the halves stays sharp: neighbours across it keep their side's level.
        let left = out[8 * w + w / 2 - 1].x;
        let right = out[8 * w + w / 2].x;
        assert!(left < 0.35 && right > 0.65, "edge kept: {left} {right}");
    }
}
