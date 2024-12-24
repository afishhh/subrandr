use crate::{
    color::BGRA8,
    math::{Fixed, Point2},
    Painter,
};

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
    pub const fn current(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub const fn new_low(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
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
    pub const fn is_done_low(&self) -> bool {
        self.x > self.x1
    }

    pub const fn advance_low(&mut self) -> bool {
        if self.d > 0 {
            self.y += self.i;
            self.d -= 2 * self.dx;
        }
        self.d += 2 * self.dy;
        self.x += 1;
        self.is_done_low()
    }

    pub const fn new_high(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
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
    pub const fn is_done_high(&self) -> bool {
        self.y > self.y1
    }

    pub const fn advance_high(&mut self) -> bool {
        if self.d > 0 {
            self.x += self.i;
            self.d -= 2 * self.dy;
        }
        self.d += 2 * self.dx;
        self.y += 1;
        self.is_done_high()
    }

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> (Self, BresenhamKind) {
        #[allow(clippy::collapsible_else_if)]
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

    pub const fn is_done(&self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.is_done_low(),
            BresenhamKind::High => self.is_done_high(),
        }
    }

    pub const fn advance(&mut self, kind: BresenhamKind) -> bool {
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
    buffer: &mut [BGRA8],
    stride: usize,
    width: i32,
    height: i32,
    color: BGRA8,
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

            let i = y as usize * stride + x as usize;
            buffer[i] = color;
        }

        if machine.advance(kind) {
            return;
        }
    }
}

pub unsafe fn horizontal_line_unchecked(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [BGRA8],
    width: i32,
    color: BGRA8,
) {
    for x in x0.clamp(0, width)..=x1.clamp(0, width) {
        offset_buffer[x as usize] = color;
    }
}

macro_rules! check_buffer {
    ($what: literal, $buffer: ident, $width: ident, $height: ident) => {
        if $buffer.len() < $width as usize * $height as usize {
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
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("line", buffer, width, height);

    unsafe {
        line_unchecked(
            x0,
            y0,
            x1,
            y1,
            buffer,
            width as usize,
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
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("horizontal_line", buffer, width, height);

    if y < 0 || y >= height as i32 {
        return;
    }

    unsafe {
        horizontal_line_unchecked(
            x0,
            x1,
            &mut buffer[y as usize * width as usize..],
            width as i32,
            color,
        )
    }
}

pub fn stroke_polygon(
    points: impl IntoIterator<Item = (i32, i32)>,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("stroke_rectangle", buffer, width, height);

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
                width as usize,
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
            width as usize,
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
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("stroke_triangle", buffer, width, height);

    let stride = width as usize;
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
    buffer: &mut [BGRA8],
    stride: usize,
    width: u32,
    height: u32,
    color: BGRA8,
) -> i32 {
    'top: loop {
        // Advance both lines until they are at the current y
        let m1x = loop {
            let (m1x, m1y) = machine1.current();
            if m1y == current_y {
                break m1x;
            } else if machine1.advance(kind1) {
                break 'top;
            }
        };
        let m2x = loop {
            let (m2x, m2y) = machine2.current();
            if m2y == current_y {
                break m2x;
            } else if machine2.advance(kind2) {
                break 'top;
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
    buffer: &mut [BGRA8],
    stride: usize,
    width: u32,
    height: u32,
    color: BGRA8,
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

const POLYGON_RASTERIZER_DEBUG_PRINT: bool = false;

// 18.14 signed fixed point value
type Fixed18 = Fixed<14>;

#[derive(Debug)]
struct Profile {
    current: Fixed18,
    step: Fixed18,
    end_y: u32,
}

#[derive(Debug)]
pub struct NonZeroPolygonRasterizer {
    queue: Vec<(u32, bool, Profile)>,
    left: Vec<Profile>,
    right: Vec<Profile>,
}

impl NonZeroPolygonRasterizer {
    pub const fn new() -> Self {
        Self {
            queue: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.left.clear();
        self.right.clear();
    }

    fn add_line(&mut self, offset: (i32, i32), start: &Point2, end: &Point2, invert_winding: bool) {
        let istart = (
            Fixed18::from_f32(start.x) + offset.0,
            Fixed18::from_f32(start.y) + offset.1,
        );
        let iend = (
            Fixed18::from_f32(end.x) + offset.0,
            Fixed18::from_f32(end.y) + offset.1,
        );

        let direction = match iend.1.cmp(&istart.1) {
            // Line is going up
            std::cmp::Ordering::Less => false ^ invert_winding,
            // Horizontal line, ignore
            std::cmp::Ordering::Equal => return,
            // Line is going down
            std::cmp::Ordering::Greater => true ^ invert_winding,
        };

        let step = if istart.0 == iend.0 {
            Fixed18::ZERO
        } else {
            (iend.0 - istart.0) / (iend.1 - istart.1)
        };

        let start_y = istart.1.round_to_i32();
        let mut start_x = istart.0;
        start_x -= (istart.1 - start_y) * step;

        let end_y = iend.1.round_to_i32();
        let mut end_x = iend.0;
        end_x -= (iend.1 - end_y) * step;

        let (mut top_y, mut bottom_y, mut init_x) = if end_y >= start_y {
            (start_y, end_y, start_x)
        } else {
            (end_y, start_y, end_x)
        };

        // FIXME: HACK: This is terrible but I tried everything and only this works
        bottom_y -= 1;
        init_x -= step;

        if top_y < 0 {
            init_x += step * -top_y;
            top_y = 0;
        }

        if top_y > bottom_y {
            return;
        }

        if POLYGON_RASTERIZER_DEBUG_PRINT {
            println!("{start_y} {end_y} {start_x} {end_x}");
            println!("{top_y} {bottom_y}");
            println!(
                "line {start:?} -- {end:?} results in top_y={top_y} direction={:?}",
                step > 0
            );
        }

        self.queue.push((
            top_y as u32,
            direction,
            Profile {
                current: init_x,
                step,
                end_y: bottom_y as u32,
            },
        ));
    }

    pub fn append_polyline(
        &mut self,
        offset: (i32, i32),
        polyline: &[Point2],
        invert_winding: bool,
    ) {
        if polyline.is_empty() {
            return;
        }

        let mut i = 0;
        while i < polyline.len() - 1 {
            let start = &polyline[i];
            i += 1;
            let end = &polyline[i];
            self.add_line(offset, start, end, invert_winding)
        }

        let last = polyline.last().unwrap();
        if &polyline[0] != last {
            self.add_line(offset, last, &polyline[0], invert_winding)
        }
    }

    fn queue_pop_if(&mut self, cy: u32) -> Option<(u32, bool, Profile)> {
        let &(y, ..) = self.queue.last()?;

        if y <= cy {
            self.queue.pop()
        } else {
            None
        }
    }

    fn push_queue_to_lr(&mut self, cy: u32) {
        while let Some((_, d, p)) = self.queue_pop_if(cy) {
            let vec = if d { &mut self.right } else { &mut self.left };
            let idx = match vec.binary_search_by_key(&p.current, |profile| profile.current) {
                Ok(i) => i,
                Err(i) => i,
            };
            vec.insert(idx, p);
        }
    }

    fn prune_lr(&mut self, cy: u32) {
        self.left.retain(|profile| profile.end_y >= cy);
        self.right.retain(|profile| profile.end_y >= cy);
    }

    fn advance_lr_sort(&mut self) {
        for profile in self.left.iter_mut() {
            profile.current += profile.step;
        }

        for profile in self.right.iter_mut() {
            profile.current += profile.step;
        }

        self.left.sort_unstable_by_key(|profile| profile.current);
        self.right.sort_unstable_by_key(|profile| profile.current);
    }

    pub fn render(&mut self, width: u32, height: u32, mut filler: impl FnMut(u32, u32, u32)) {
        self.queue.sort_unstable_by(|(ay, ..), (by, ..)| by.cmp(ay));

        if self.queue.is_empty() {
            return;
        }

        let mut y = self.queue.last().unwrap().0;

        while (!self.queue.is_empty() || !self.left.is_empty()) && y < height {
            self.prune_lr(y);
            self.push_queue_to_lr(y);

            if POLYGON_RASTERIZER_DEBUG_PRINT {
                println!("--- POLYLINE RASTERIZER SCANLINE y={y} ---");
                println!("left: {:?}", self.left);
                println!("right: {:?}", self.right);
                assert_eq!(self.left.len(), self.right.len());
            }

            for i in 0..self.left.len() {
                let (left, right) = (&self.left[i], &self.right[i]);

                let round_clamp = |f: Fixed18| (f.round_to_i32().max(0) as u32).min(width);
                let mut x0 = round_clamp(left.current);
                let mut x1 = round_clamp(right.current);
                // TODO: is this necessary? can this be removed?
                if x0 > x1 {
                    std::mem::swap(&mut x0, &mut x1);
                }
                filler(y, x0, x1);
            }

            self.advance_lr_sort();

            y += 1;
        }
    }

    // TODO: Move to painter
    pub fn render_fill(&mut self, painter: &mut Painter, color: BGRA8) {
        self.render(painter.width(), painter.height(), |y, x1, x2| {
            painter.horizontal_line(y as i32, x1 as i32, x2 as i32, color);
        });
    }
}
