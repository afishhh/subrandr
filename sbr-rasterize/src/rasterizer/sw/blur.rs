use std::{f32::consts::PI, mem::MaybeUninit};

pub fn gaussian_sigma_to_box_radius(sigma: f32) -> usize {
    // https://drafts.fxtf.org/filter-effects/#funcdef-filter-blur
    // Divided by two because we want a *radius* not the whole "extent".
    (((2.0 * PI).sqrt() * 0.375) * sigma).round() as usize
}

pub enum BlurKernel {
    Box(BoxBlurKernel),
}

pub struct BoxBlurKernel {
    radius: usize,
    reciprocal_extent: f32,
}

impl BoxBlurKernel {
    pub fn new(radius: usize) -> Self {
        Self {
            radius,
            reciprocal_extent: ((2 * radius + 1) as f32).recip(),
        }
    }

    pub fn from_gaussian_stddev(stddev: f32) -> Self {
        Self::new(gaussian_sigma_to_box_radius(stddev))
    }

    #[inline(always)]
    unsafe fn run(&self, front: *const f32, back: *mut f32, stride: usize, length: usize) {
        let mut sum = 0.0;
        let mut x = 0;
        for _ in 0..self.radius {
            sum += unsafe { *front.add(x * stride) };
        }

        while x < self.radius {
            sum += unsafe { *front.add((x + self.radius) * stride) };
            unsafe { back.add(x * stride).write(sum * self.reciprocal_extent) };
            x += 1;
        }

        while x < length - self.radius {
            sum += unsafe { *front.add((x + self.radius) * stride) };
            unsafe { back.add(x * stride).write(sum * self.reciprocal_extent) };
            sum -= unsafe { *front.add((x - self.radius) * stride) };
            x += 1;
        }

        while x < length {
            unsafe { back.add(x * stride).write(sum * self.reciprocal_extent) };
            sum -= unsafe { *front.add((x - self.radius) * stride) };
            x += 1;
        }
    }
}

impl BlurKernel {
    pub fn radius(&self) -> usize {
        match self {
            Self::Box(box_kernel) => box_kernel.radius,
        }
    }

    pub fn padding(&self) -> usize {
        2 * self.radius()
    }

    #[inline(always)]
    unsafe fn run(&self, front: *const f32, back: *mut f32, stride: usize, length: usize) {
        match self {
            Self::Box(box_kernel) => box_kernel.run(front, back, stride, length),
        }
    }
}

pub struct Blurer {
    front: Vec<f32>,
    back: Vec<MaybeUninit<f32>>,
    width: usize,
    height: usize,
    kernel: BlurKernel,
}

impl Blurer {
    pub fn new() -> Self {
        Self {
            front: Vec::new(),
            back: Vec::new(),
            width: 0,
            height: 0,
            kernel: BlurKernel::Box(BoxBlurKernel::new(0)),
        }
    }

    pub fn prepare(&mut self, width: usize, height: usize, kernel: BlurKernel) {
        let twidth = width + 2 * kernel.padding();
        let theight = height + 2 * kernel.padding();
        let size = twidth * theight;

        self.front.clear();
        self.front.resize(size, 0.0);
        self.back.resize(size, MaybeUninit::uninit());

        self.width = twidth;
        self.height = theight;
        self.kernel = kernel;
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

    pub fn blur_horizontal(&mut self) {
        for y in 0..self.height {
            unsafe {
                self.kernel.run(
                    self.front.as_ptr().add(y * self.width),
                    self.back.as_mut_ptr().add(y * self.width).cast(),
                    1,
                    self.width,
                );
            }
        }

        unsafe { self.swap_buffers() };
    }

    pub fn blur_vertical(&mut self) {
        for x in 0..self.width {
            unsafe {
                self.kernel.run(
                    self.front.as_ptr().add(x),
                    self.back.as_mut_ptr().add(x).cast(),
                    self.width,
                    self.height,
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
        self.kernel.padding()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}
