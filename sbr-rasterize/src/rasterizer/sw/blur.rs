use std::{f32::consts::PI, mem::MaybeUninit};

pub fn gaussian_sigma_to_box_radius(sigma: f32) -> usize {
    // https://drafts.fxtf.org/filter-effects/#funcdef-filter-blur
    // Divided by two because we want a *radius* not the whole "extent".
    (((2.0 * PI).sqrt() * 0.375) * sigma).round() as usize
}

const PADDING_RADIUS: usize = 2;

#[inline(always)]
unsafe fn sliding_sum(
    front: *const f32,
    back: *mut f32,
    stride: usize,
    length: usize,
    radius: usize,
    iextent: f32,
) {
    let mut sum = 0.0;
    let mut x = 0;
    for _ in 0..radius {
        sum += unsafe { *front.add(x * stride) };
    }

    while x < radius {
        sum += unsafe { *front.add((x + radius) * stride) };
        unsafe { back.add(x * stride).write(sum * iextent) };
        x += 1;
    }

    while x < length - radius {
        sum += unsafe { *front.add((x + radius) * stride) };
        unsafe { back.add(x * stride).write(sum * iextent) };
        sum -= unsafe { *front.add((x - radius) * stride) };
        x += 1;
    }

    while x < length {
        unsafe { back.add(x * stride).write(sum * iextent) };
        sum -= unsafe { *front.add((x - radius) * stride) };
        x += 1;
    }
}

// TODO: Is using fixed point arithmetic here worth it?
pub struct Blurer {
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

    pub fn prepare(&mut self, width: usize, height: usize, radius: usize) {
        let twidth = width + 2 * PADDING_RADIUS * radius;
        let theight = height + 2 * PADDING_RADIUS * radius;
        let size = twidth * theight;

        self.front.clear();
        self.front.resize(size, 0.0);
        self.back.resize(size, MaybeUninit::uninit());

        self.width = twidth;
        self.height = theight;
        self.radius = radius;
        self.iextent = ((radius * 2 + 1) as f32).recip();
    }

    unsafe fn swap_buffers(&mut self) {
        let (front_ptr, front_len, front_capacity) = util::vec_parts(&mut self.front);
        let (back_ptr, back_len, back_capacity) = util::vec_parts(&mut self.back);
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

    pub fn box_blur_horizontal(&mut self) {
        for y in 0..self.height {
            unsafe {
                sliding_sum(
                    self.front.as_ptr().add(y * self.width),
                    self.back.as_mut_ptr().add(y * self.width).cast(),
                    1,
                    self.width,
                    self.radius,
                    self.iextent,
                );
            }
        }

        unsafe { self.swap_buffers() };
    }

    pub fn box_blur_vertical(&mut self) {
        for x in 0..self.width {
            unsafe {
                sliding_sum(
                    self.front.as_ptr().add(x),
                    self.back.as_mut_ptr().add(x).cast(),
                    self.width,
                    self.height,
                    self.radius,
                    self.iextent,
                );
            }
        }

        unsafe { self.swap_buffers() };
    }

    pub fn front(&self) -> &[f32] {
        &self.front
    }

    pub fn front_mut(&mut self) -> &mut [f32] {
        &mut self.front
    }

    pub fn padding(&self) -> usize {
        PADDING_RADIUS * self.radius
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}
