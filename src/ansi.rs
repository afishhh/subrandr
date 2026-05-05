use std::{fmt::Display, fmt::Write};

pub struct SgrSequenceBuilder<'s> {
    output: &'s mut String,
    started: bool,
}

impl<'s> SgrSequenceBuilder<'s> {
    pub fn new(output: &'s mut String) -> Self {
        Self {
            output,
            started: false,
        }
    }

    fn push_sgr_argument(&mut self, x: impl Display) {
        if !self.started {
            self.output.push_str("\x1b[");
            _ = write!(self.output, "{}", x);
            self.started = true;
        } else {
            self.output.push(';');
            _ = write!(self.output, "{}", x);
        }
    }

    pub fn set_bold(&mut self) {
        self.push_sgr_argument("1");
    }

    pub fn set_italic(&mut self) {
        self.push_sgr_argument("3");
    }

    pub fn set_underline(&mut self, color: Option<(u8, u8, u8)>) {
        if let Some((r, g, b)) = color {
            self.push_sgr_argument("58");
            self.push_sgr_argument("2");
            self.push_sgr_argument(r);
            self.push_sgr_argument(g);
            self.push_sgr_argument(b);
        } else {
            self.push_sgr_argument("59");
        }
        self.push_sgr_argument("4");
    }

    pub fn set_foreground_color(&mut self, r: u8, g: u8, b: u8) {
        self.push_sgr_argument("38");
        self.push_sgr_argument("2");
        self.push_sgr_argument(r);
        self.push_sgr_argument(g);
        self.push_sgr_argument(b);
    }

    pub fn set_background_color(&mut self, r: u8, g: u8, b: u8) {
        self.push_sgr_argument("48");
        self.push_sgr_argument("2");
        self.push_sgr_argument(r);
        self.push_sgr_argument(g);
        self.push_sgr_argument(b);
    }

    pub fn reset_all(&mut self) {
        self.push_sgr_argument("0");
    }
}

impl Drop for SgrSequenceBuilder<'_> {
    fn drop(&mut self) {
        if self.started {
            self.output.push('m');
        }
    }
}
