#[derive(Debug, Clone)]
struct Bresenham {
    dx: i32,
    dy: i32,
    // Either xi or yi
    i: i32,
    d: i32,

    x: i32,
    y: i32,
    x1: i32,
    y1: i32,
}

#[derive(Debug, Clone, Copy)]
enum BresenhamKind {
    Low,
    High,
}

impl Bresenham {
    #[inline(always)]
    pub fn current(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub fn new_low(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let dx = x1 - x0;
        let mut dy = y1 - y0;
        let mut yi = 1;

        if dy < 0 {
            yi = -1;
            dy = -dy;
        }

        let d = 2 * dy - dx;
        let y = y0;

        Self {
            dx,
            dy,
            i: yi,
            d,
            x: x0,
            y,
            x1,
            y1,
        }
    }

    #[inline(always)]
    pub fn is_done_low(&self) -> bool {
        self.x > self.x1
    }

    pub fn advance_low(&mut self) -> bool {
        if self.d > 0 {
            self.y += self.i;
            self.d -= 2 * self.dx;
        }
        self.d += 2 * self.dy;
        self.x += 1;
        return self.is_done_low();
    }

    pub fn new_high(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let mut dx = x1 - x0;
        let dy = y1 - y0;
        let mut xi = 1;

        if dx < 0 {
            xi = -1;
            dx = -dx;
        }

        let d = 2 * dx - dy;
        let x = x0;

        Self {
            dx,
            dy,
            i: xi,
            d,
            x,
            y: y0,
            x1,
            y1,
        }
    }

    #[inline(always)]
    pub fn is_done_high(&self) -> bool {
        self.y > self.y1
    }

    pub fn advance_high(&mut self) -> bool {
        if self.d > 0 {
            self.x += self.i;
            self.d -= 2 * self.dy;
        }
        self.d += 2 * self.dx;
        self.y += 1;
        return self.is_done_high();
    }

    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> (Self, BresenhamKind) {
        if (y1 - y0).abs() < (x1 - x0).abs() {
            if x0 > x1 {
                (Self::new_low(x1, y1, x0, y0), BresenhamKind::Low)
            } else {
                (Self::new_low(x0, y0, x1, y1), BresenhamKind::Low)
            }
        } else {
            if y0 > y1 {
                (Self::new_high(x1, y1, x0, y0), BresenhamKind::High)
            } else {
                (Self::new_high(x0, y0, x1, y1), BresenhamKind::High)
            }
        }
    }

    pub fn is_done(&self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.is_done_low(),
            BresenhamKind::High => self.is_done_high(),
        }
    }

    pub fn advance(&mut self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.advance_low(),
            BresenhamKind::High => self.advance_high(),
        }
    }
}

pub unsafe fn line_unchecked(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    buffer: &mut [u8],
    stride: usize,
    width: i32,
    height: i32,
    color: u32,
) {
    let (mut machine, kind) = Bresenham::new(x0, y0, x1, y1);
    loop {
        let (x, y) = machine.current();

        'a: {
            if y < 0 || y >= height {
                break 'a;
            }

            if x < 0 || x >= width {
                break 'a;
            }

            let i = y as usize * stride + 4 * (x as usize);
            let pixel = unsafe {
                <&mut [u8; 4]>::try_from(buffer.get_unchecked_mut(i..i + 4)).unwrap_unchecked()
            };
            *pixel = color.to_be_bytes();
        }

        if machine.advance(kind) {
            return;
        }
    }
}

pub unsafe fn horizontal_line_unchecked(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [u8],
    width: i32,
    color: u32,
) {
    for x in x0.clamp(0, width)..=x1.clamp(0, width) {
        let i = 4 * (x as usize);
        let pixel = unsafe {
            <&mut [u8; 4]>::try_from(offset_buffer.get_unchecked_mut(i..i + 4)).unwrap_unchecked()
        };
        *pixel = color.to_be_bytes();
    }
}

macro_rules! check_buffer {
    ($what: literal, $buffer: ident, $width: ident, $height: ident) => {
        if $buffer.len() < $width as usize * $height as usize * 4 {
            panic!(concat!(
                "Buffer passed to rasterize::",
                $what,
                " is too small"
            ))
        }
    };
}

pub fn line(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    color: u32,
) {
    check_buffer!("line", buffer, width, height);

    unsafe {
        line_unchecked(
            x0,
            y0,
            x1,
            y1,
            buffer,
            4 * width as usize,
            width as i32,
            height as i32,
            color,
        )
    }
}

pub fn horizontal_line(
    y: i32,
    x0: i32,
    x1: i32,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    color: u32,
) {
    check_buffer!("horizontal_line", buffer, width, height);

    if y < 0 || y >= height as i32 {
        return;
    }

    unsafe {
        horizontal_line_unchecked(
            x0,
            x1,
            &mut buffer[y as usize * width as usize * 4..],
            width as i32,
            color,
        )
    }
}

pub fn stroke_polygon(
    points: impl IntoIterator<Item = (i32, i32)>,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    color: u32,
) {
    check_buffer!("stroke_rectangle", buffer, width, height);

    let stride = 4 * width as usize;
    let mut it = points.into_iter();
    let Some(first) = it.next() else {
        return;
    };

    let mut last = first;
    for next in it {
        unsafe {
            line_unchecked(
                last.0,
                last.1,
                next.0,
                next.1,
                buffer,
                stride,
                width as i32,
                height as i32,
                color,
            );
        }

        last = next;
    }

    unsafe {
        line_unchecked(
            last.0,
            last.1,
            first.0,
            first.1,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }
}

pub fn stroke_triangle(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    color: u32,
) {
    check_buffer!("stroke_triangle", buffer, width, height);

    let stride = 4 * width as usize;
    unsafe {
        line_unchecked(
            x0,
            y0,
            x1,
            y1,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }

    unsafe {
        line_unchecked(
            x1,
            y1,
            x2,
            y2,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }

    unsafe {
        line_unchecked(
            x0,
            y0,
            x2,
            y2,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }
}

unsafe fn draw_triangle_half(
    mut current_y: i32,
    machine1: &mut Bresenham,
    kind1: BresenhamKind,
    machine2: &mut Bresenham,
    kind2: BresenhamKind,
    buffer: &mut [u8],
    stride: usize,
    width: u32,
    height: u32,
    color: u32,
) -> i32 {
    'top: loop {
        // Advance both lines until they are at the current y
        let m1x = loop {
            let (m1x, m1y) = machine1.current();
            if m1y == current_y {
                break m1x;
            } else {
                if machine1.advance(kind1) {
                    break 'top;
                }
            }
        };
        let m2x = loop {
            let (m2x, m2y) = machine2.current();
            if m2y == current_y {
                break m2x;
            } else {
                if machine2.advance(kind2) {
                    break 'top;
                }
            }
        };

        // Fill the appropriate part of the line at the current y
        if current_y >= 0 && current_y < height as i32 {
            let (lx1, lx2) = if m1x < m2x { (m1x, m2x) } else { (m2x, m1x) };

            unsafe {
                horizontal_line_unchecked(
                    lx1,
                    lx2,
                    &mut buffer[current_y as usize * stride..],
                    width as i32,
                    color,
                );
            }
        }

        current_y += 1;
    }
    current_y
}

pub fn fill_triangle(
    mut x0: i32,
    mut y0: i32,
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    buffer: &mut [u8],
    width: u32,
    height: u32,
    color: u32,
) {
    check_buffer!("fill_triangle", buffer, width, height);

    // First, ensure (x0, y0) is the highest point of the triangle
    if y1 < y0 {
        if y2 < y1 {
            std::mem::swap(&mut y0, &mut y2);
            std::mem::swap(&mut x0, &mut x2);
        } else {
            std::mem::swap(&mut y0, &mut y1);
            std::mem::swap(&mut x0, &mut x1);
        }
    } else if y2 < y0 {
        std::mem::swap(&mut y0, &mut y2);
        std::mem::swap(&mut x0, &mut x2);
    }

    // Next, ensure (x2, y2) is the lowest point of the rectangle
    if y1 > y2 {
        std::mem::swap(&mut y2, &mut y1);
        std::mem::swap(&mut x2, &mut x1);
    }

    let stride = 4 * width as usize;

    let (mut machine1, kind1) = Bresenham::new(x0, y0, x1, y1);
    let (mut machine2, kind2) = Bresenham::new(x0, y0, x2, y2);

    let mut current_y = y0;
    current_y = unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            kind1,
            &mut machine2,
            kind2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };

    let (mut machine1, kind1) = {
        if machine1.is_done(kind1) {
            (machine2, kind2)
        } else {
            (machine1, kind1)
        }
    };
    let (mut machine2, kind2) = Bresenham::new(x1, y1, x2, y2);

    unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            kind1,
            &mut machine2,
            kind2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };
}
