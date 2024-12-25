use crate::color::{BlendMode, BGRA8};

fn calculate_gassian_kernel(sigma: f32) -> Vec<f32> {
    let size = (sigma * 3.0).ceil() as usize;
    let mut kernel = vec![0.0; size * 2 + 1];

    let sigma_sq = sigma * sigma;
    let factor = (sigma_sq * const { 2.0 * std::f32::consts::PI })
        .sqrt()
        .recip();
    let sigmasq2inv = (2.0 * sigma_sq).recip();
    for x in -(size as isize)..=size as isize {
        kernel[(x + size as isize) as usize] =
            factor * std::f32::consts::E.powf((-x * x) as f32 * sigmasq2inv);
    }

    // normalize the kernel to avoid darkening of the blurred image
    let sum = kernel.iter().sum::<f32>();
    for v in kernel.iter_mut() {
        *v /= sum;
    }

    kernel
}

// TODO: this function *could* have less parameters, but also I got lazy so no
pub fn monochrome_gaussian_blit(
    sigma: f32,
    x: isize,
    y: isize,
    target: &mut [BGRA8],
    target_width: usize,
    target_height: usize,
    source: &[u8],
    source_width: usize,
    source_height: usize,
    color: [u8; 3],
    blend: BlendMode,
) {
    let kernel = calculate_gassian_kernel(sigma);
    let buffer_width = source_width + kernel.len();
    let buffer_height = source_height + kernel.len();
    let mut buffer = vec![0.0; buffer_width * buffer_height];
    let pad = (kernel.len() >> 1) as isize;

    for sy in 0..source_height + kernel.len() {
        for sx in 0..source_width + kernel.len() {
            let mut khere = 0.0;
            for iy in -pad..=pad {
                let ky = kernel[(iy + pad) as usize];
                khere += if let Some(cy) = sy
                    .checked_add_signed(iy - pad)
                    .filter(|&y| y < source_height)
                {
                    if let Some(cx) = sx.checked_add_signed(-pad).filter(|&x| x < source_width) {
                        ky * (source[cy * source_width + cx] as f32 / 255.)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };
            }

            buffer[sy * buffer_width + sx] = khere;
        }
    }

    let nx = x - pad;
    let ny = y - pad;
    for sy in 0..source_height + kernel.len() {
        for sx in 0..source_width + kernel.len() {
            let mut khere = 0.0;
            for ix in -pad..=pad {
                let kx = kernel[(ix + pad) as usize];
                khere += if let Some(cx) = sx.checked_add_signed(ix).filter(|&x| x < buffer_width) {
                    kx * buffer[sy * buffer_width + cx]
                } else {
                    0.0
                };
            }

            let dy = ny + sy as isize;
            let dx = nx + sx as isize;
            if dy < 0 || dy >= target_height as isize || dx < 0 || dx >= target_width as isize {
                continue;
            }

            let di = (dy as usize) * target_width + dx as usize;
            blend.blend_with_parts(
                &mut target[di],
                color.map(|c| (c as f32 * khere) as u8),
                khere,
            );
        }
    }
}
