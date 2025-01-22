use std::{f32::consts::PI, mem::MaybeUninit};

use crate::{
    color::BGRA8,
    util::{calculate_blit_rectangle, vec_parts, BlitRectangle},
};

fn gaussian_sigma_to_box_radius(sigma: f32) -> usize {
    // https://drafts.fxtf.org/filter-effects/#funcdef-filter-blur
    // Divided by two because we want a *radius* not the whole "extent".
    (((2.0 * PI).sqrt() * 0.375) * sigma).round() as usize
}

const PADDING_RADIUS: usize = 2;

// FIXME: I wonder whether this gets as inlined as I want it to be
#[inline(always)]
fn sliding_sum(
    front: &[f32],
    back: &mut [MaybeUninit<f32>],
    stride: usize,
    length: usize,
    radius: usize,
    iextent: f32,
) {
    let mut sum = 0.0;
    let mut x = 0;
    for _ in 0..radius {
        sum += front[x * stride];
    }

    while x < 2 * radius {
        back[x * stride].write(sum * iextent);
        sum += front[(x + radius) * stride];
        x += 1;
    }

    while x < length - radius {
        sum += front[(x + radius) * stride];
        back[x * stride].write(sum * iextent);
        sum -= front[(x - radius) * stride];
        x += 1;
    }

    while x < length {
        back[x * stride].write(sum * iextent);
        sum -= front[(x - radius) * stride];
        x += 1;
    }
}

// TODO: Is using fixed point arithmetic here worth it?
struct Blurer {
    front: Vec<f32>,
    back: Vec<MaybeUninit<f32>>,
    width: usize,
    height: usize,
    radius: usize,
    iextent: f32,
}

impl Blurer {
    pub fn new() -> Self {
        Self {
            front: Vec::new(),
            back: Vec::new(),
            width: 0,
            height: 0,
            radius: 0,
            iextent: 0.0,
        }
    }

    fn prepare(&mut self, width: usize, height: usize, radius: usize, source: &[u8]) {
        let twidth = width + 2 * PADDING_RADIUS * radius;
        let theight = height + 2 * PADDING_RADIUS * radius;
        let size = twidth * theight;

        {
            self.front.clear();
            self.front.reserve(size);
            let front = self.front.spare_capacity_mut();

            for y in 0..PADDING_RADIUS * radius {
                front[y * twidth..(y + 1) * twidth].fill(MaybeUninit::new(0.0))
            }

            for y in (height + PADDING_RADIUS * radius)..theight {
                front[y * twidth..(y + 1) * twidth].fill(MaybeUninit::new(0.0))
            }

            for y in PADDING_RADIUS * radius..(height + PADDING_RADIUS * radius) {
                let row = &mut front[y * twidth..(y + 1) * twidth];
                row[..PADDING_RADIUS * radius].fill(MaybeUninit::new(0.0));
                row[(width + PADDING_RADIUS * radius)..].fill(MaybeUninit::new(0.0));
            }

            for y in (height + PADDING_RADIUS * radius)..theight {
                front[y * twidth..(y + 1) * twidth].fill(MaybeUninit::new(0.0))
            }

            for y in 0..height {
                for x in 0..width {
                    front[(y + PADDING_RADIUS * radius) * twidth + x + PADDING_RADIUS * radius]
                        .write(source[y * width + x] as f32 / 255.0);
                }
            }

            unsafe { self.front.set_len(size) };
        }

        self.back.resize(size, MaybeUninit::uninit());
        self.width = twidth;
        self.height = theight;
        self.radius = radius;
        self.iextent = ((radius * 2 + 1) as f32).recip();
    }

    unsafe fn swap_buffers(&mut self) {
        let (front_ptr, front_len, front_capacity) = vec_parts(&mut self.front);
        let (back_ptr, back_len, back_capacity) = vec_parts(&mut self.back);
        std::ptr::write(
            &mut self.front,
            Vec::from_raw_parts(back_ptr as *mut f32, back_len, back_capacity),
        );
        std::ptr::write(
            &mut self.back,
            Vec::from_raw_parts(
                front_ptr as *mut MaybeUninit<f32>,
                front_len,
                front_capacity,
            ),
        );
    }

    fn box_blur_horizontal(&mut self) {
        for y in 0..self.height {
            sliding_sum(
                &self.front[y * self.width..(y + 1) * self.width],
                &mut self.back[y * self.width..(y + 1) * self.width],
                1,
                self.width,
                self.radius,
                self.iextent,
            );
        }

        unsafe { self.swap_buffers() };
    }

    fn box_blur_vertical(&mut self) {
        for x in 0..self.width {
            sliding_sum(
                &self.front[x..],
                &mut self.back[x..],
                self.width,
                self.height,
                self.radius,
                self.iextent,
            );
        }

        unsafe { self.swap_buffers() };
    }

    pub fn box_blur(&mut self, source: &[u8], width: usize, height: usize, radius: usize) {
        self.prepare(width, height, radius, source);
        self.box_blur_horizontal();
        self.box_blur_vertical();
    }

    pub fn box_blur3(&mut self, source: &[u8], width: usize, height: usize, radius: usize) {
        self.prepare(width, height, radius, source);
        self.box_blur_horizontal();
        self.box_blur_horizontal();
        self.box_blur_horizontal();
        self.box_blur_vertical();
        self.box_blur_vertical();
        self.box_blur_vertical();
    }

    pub fn front(&self) -> &[f32] {
        &self.front
    }

    pub fn radius(&self) -> usize {
        self.radius
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

pub fn monochrome_gaussian_blit(
    sigma: f32,
    x: i32,
    y: i32,
    target: &mut [BGRA8],
    target_width: usize,
    target_height: usize,
    source: &[u8],
    source_width: usize,
    source_height: usize,
    color: [u8; 3],
) {
    let radius = gaussian_sigma_to_box_radius(sigma);
    let tox = x - (PADDING_RADIUS * radius) as i32;
    let toy = y - (PADDING_RADIUS * radius) as i32;
    let Some(BlitRectangle { xs, ys }) = calculate_blit_rectangle(
        tox,
        toy,
        target_width,
        target_height,
        source_width + 2 * PADDING_RADIUS * radius,
        source_height + 2 * PADDING_RADIUS * radius,
    ) else {
        return;
    };

    let mut blurer = Blurer::new();
    blurer.box_blur3(source, source_width, source_height, radius);

    let blurred = blurer.front();

    for sy in ys {
        let mut ti = (((sy as i32 + toy) as usize * target_width) as isize + tox as isize) as usize;
        for sx in xs.clone() {
            let mut khere = blurred[sy * blurer.width + sx];
            khere = khere.clamp(0.0, 1.0);

            let c = BGRA8::from_bytes([color[0], color[1], color[2], (khere * 255.0) as u8]);
            target[ti] = c.blend_over(target[ti]).0;
            ti += 1;
        }
    }
}
