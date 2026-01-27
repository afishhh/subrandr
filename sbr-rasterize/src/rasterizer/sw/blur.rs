use std::{f32::consts::PI, mem::MaybeUninit};

pub fn gaussian_sigma_to_box_radius(sigma: f32) -> usize {
    // https://drafts.fxtf.org/filter-effects/#funcdef-filter-blur
    // Divided by two because we want a *radius* not the whole "extent".
    (((2.0 * PI).sqrt() * 0.375) * sigma).round() as usize
}

pub enum BlurKernel {
    Box(BoxBlurKernel),
    Gaussian(GaussianBlurKernel),
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

pub struct GaussianBlurKernel {
    values: Vec<f32>,
}

impl GaussianBlurKernel {
    pub fn new(stddev: f32) -> Self {
        let radius = (3. * stddev).ceil() as usize;
        let mut values = Vec::with_capacity(radius + 1);

        let stddev_sq = stddev * stddev;
        let scale = (2. * PI * stddev_sq).sqrt().recip();
        let exponent_scale = -(2. * stddev_sq).recip();
        let sample = |x: f32| scale * (x * x * exponent_scale).exp();

        // We center the kernel at the midpoint of the center pixel so
        // the first pixel's argument range is going to be [-0.5, 0.5].
        // Subsequent pixels will follow the same pattern.
        let mut last = -0.5;
        let mut last_sample = sample(last);
        let mut sum = 0.0;
        for _ in 0..values.capacity() {
            const STEPS: u32 = 2;
            const STEP: f32 = 1.0 / (STEPS as f32);
            const HALF_STEP: f32 = STEP / 2.;

            let mut result = 0.0;
            // Do an approximate integration by summing up the area of two trapezoids
            for _ in 0..STEPS {
                let end = last + STEP;
                let end_sample = sample(end);

                result += (last_sample + end_sample) * HALF_STEP;

                last_sample = end_sample;
                last = end;
            }

            values.push(result);
            sum += result;
        }

        // Normalize the kernel to make sure everything sums to one.
        // Note that we only store one half (as the kernel is symmetrical) so
        // account for that when calculating our sum.
        let sum_recip = (sum - values[0] + sum).recip();
        for value in values.iter_mut() {
            *value *= sum_recip;
        }

        Self { values }
    }

    pub fn radius(&self) -> usize {
        self.values.len() - 1
    }

    #[inline(always)]
    unsafe fn run(&self, front: *const f32, back: *mut f32, stride: usize, length: usize) {
        let radius = self.radius();
        let mut cx = 0;

        while cx < radius {
            let mut result = 0.0;
            let mut x = 0;
            let mut vi = cx;

            while vi > 0 {
                result += unsafe { *front.add(x * stride) } * self.values[vi];
                vi -= 1;
                x += 1;
            }
            result += unsafe { *front.add(x * stride) } * self.values[0];
            while vi < radius {
                vi += 1;
                x += 1;
                result += unsafe { *front.add(x * stride) } * self.values[vi];
            }

            unsafe { back.add(cx * stride).write(result) };
            cx += 1;
        }

        while cx < length - radius {
            let mut result = 0.0;
            let mut x = cx;
            let mut vi = 0;

            while vi < radius {
                vi += 1;
                x -= 1;
                result += unsafe { *front.add(x * stride) } * self.values[vi];
            }
            x = cx;
            vi = 0;
            result += unsafe { *front.add(x * stride) } * self.values[0];
            while vi < radius {
                vi += 1;
                x += 1;
                result += unsafe { *front.add(x * stride) } * self.values[vi];
            }

            unsafe { back.add(cx * stride).write(result) };
            cx += 1;
        }

        while cx < length {
            let mut result = 0.0;
            let mut x = cx;
            let mut vi = 0;

            while vi < radius {
                vi += 1;
                x -= 1;
                result += unsafe { *front.add(x * stride) } * self.values[vi];
            }
            x = cx;
            vi = 1;
            result += unsafe { *front.add(x * stride) } * self.values[0];
            while vi < length - cx {
                x += 1;
                result += unsafe { *front.add(x * stride) } * self.values[vi];
                vi += 1;
            }

            unsafe { back.add(cx * stride).write(result) };
            cx += 1;
        }
    }
}

impl BlurKernel {
    pub fn radius(&self) -> usize {
        match self {
            Self::Box(box_kernel) => box_kernel.radius,
            Self::Gaussian(gaussian_kernel) => gaussian_kernel.radius(),
        }
    }

    pub fn padding(&self) -> usize {
        2 * self.radius()
    }

    #[inline(always)]
    unsafe fn run(&self, front: *const f32, back: *mut f32, stride: usize, length: usize) {
        match self {
            Self::Box(box_kernel) => box_kernel.run(front, back, stride, length),
            Self::Gaussian(gaussian_kernel) => gaussian_kernel.run(front, back, stride, length),
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
