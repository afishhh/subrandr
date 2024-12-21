use std::{
    fmt::{Debug, Display},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign},
};

// signed 32bit fixed point number
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed<const P: u32>(i32);

impl<const P: u32> Fixed<P> {
    pub const fn new(value: i32) -> Self {
        Self(value << P)
    }

    pub const fn from_quotient(numerator: i32, denominator: i32) -> Self {
        Self::from_quotient64(numerator as i64, denominator as i64)
    }

    pub const fn from_quotient64(numerator: i64, denominator: i64) -> Self {
        Self(((numerator << P) / denominator) as i32)
    }

    pub const fn from_raw(value: i32) -> Self {
        Self(value)
    }

    pub const fn into_raw(self) -> i32 {
        self.0
    }

    pub const fn from_f32(value: f32) -> Self {
        Self((value * (1 << P) as f32) as i32)
    }

    pub const fn into_f32(self) -> f32 {
        self.0 as f32 / (1 << P) as f32
    }

    pub const fn trunc_to_i32(self) -> i32 {
        self.0 >> P
    }

    pub const fn trunc(self) -> Self {
        let signed_floor = (self.0 >> P) << P;
        let was_negative_with_fract =
            (self.0 & (Self::FRACTIONAL_MASK | Self::SIGN_MASK)) as u32 > Self::SIGN_MASK as u32;
        Self(signed_floor + Self::ONE.0 * (was_negative_with_fract as i32))
    }

    pub const fn fract(self) -> Self {
        let unsigned = self.0 & Self::FRACTIONAL_MASK;
        let mut extension = (self.0 & Self::SIGN_MASK) >> (i32::BITS - P - 1);
        extension *= (unsigned > 0) as i32;
        Self(unsigned | extension)
    }

    pub const fn round(self) -> Self {
        let fract = self.fract();
        if fract.0 >= Self::HALF.0 {
            Self((self.0 & Self::WHOLE_MASK) + Self::ONE.0)
        } else if fract.0 <= -Self::HALF.0 {
            Self(self.0 & Self::WHOLE_MASK)
        } else {
            self.trunc()
        }
    }

    pub const fn round_to_i32(self) -> i32 {
        self.round().trunc_to_i32()
    }

    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub const fn signum(&self) -> i32 {
        // sign bit is at 1 << i32::BITS
        self.0 >> (i32::BITS - 1 - P)
    }

    pub const ONE: Self = Self(1 << P);
    pub const ZERO: Self = Self(0);
    const HALF: Self = Self(1 << (P - 1));
    const EPS: Self = Self(1);

    const SIGN_MASK: i32 = 1 << (i32::BITS - 1);
    const FRACTIONAL_MASK: i32 = (1 << P) - 1;
    const WHOLE_MASK: i32 = !Self::FRACTIONAL_MASK;
}

macro_rules! define_simple_fixed_operator {
    (@all $($tt: tt)*) => {
        define_simple_fixed_operator!(
            [Self],
            $($tt)*
        );
        define_simple_fixed_operator!(
            @conversions
            $($tt)*
        );
    };
    (@conversions $($tt: tt)*) => {
        define_simple_fixed_operator!(
            [i32 Self::new],
            $($tt)*
        );
        define_simple_fixed_operator!(
            [f32 Self::from_f32],
            $($tt)*
        );
    };
    (
        [$type: ident $($construct: tt)*],
        $trait: ident,
        $f: ident,
        $op: tt,
        $trait_assign: ident,
        $f_assign: ident,
        $op_assign: tt
    ) => {
        impl<const P: u32> $trait<$type> for Fixed<P> {
            type Output = Self;

            fn $f(self, rhs: $type) -> Self::Output {
                Self(self.0 $op $($construct)*(rhs).0)
            }
        }

        impl<const P: u32> $trait_assign<$type> for Fixed<P> {
            fn $f_assign(&mut self, rhs: $type) {
                self.0 $op_assign $($construct)*(rhs).0
            }
        }
    };
}

define_simple_fixed_operator!(
    @all Add, add, +, AddAssign, add_assign, +=
);

define_simple_fixed_operator!(
    @all Sub, sub, -, SubAssign, sub_assign, -=
);

// TODO: Both Div and Mul can be implemented more efficiently when
//       multiplying by integers
define_simple_fixed_operator!(
    @conversions Div, div, /, DivAssign, div_assign, /=
);

define_simple_fixed_operator!(
    @conversions Mul, mul, *, MulAssign, mul_assign, *=
);

impl<const P: u32> Mul for Fixed<P> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        assert!(P % 2 == 0);
        Self(((self.0 as i64 * rhs.0 as i64) >> P) as i32)
    }
}

impl<const P: u32> Div for Fixed<P> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        let wide_result = ((self.0 as i64) << P) / rhs.0 as i64;
        Self(wide_result as i32)
    }
}

impl<const P: u32> Neg for Fixed<P> {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl<const P: u32> PartialEq<i32> for Fixed<P> {
    fn eq(&self, other: &i32) -> bool {
        (self.0 & Self::FRACTIONAL_MASK) == 0 && (self.0 >> P) == *other
    }
}

impl<const P: u32> PartialOrd<i32> for Fixed<P> {
    // TODO: Full precision
    fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&(other << P))
    }
}

impl<const P: u32> From<Fixed<P>> for f32 {
    fn from(value: Fixed<P>) -> Self {
        value.into_f32()
    }
}

impl<const P: u32> From<f32> for Fixed<P> {
    fn from(value: f32) -> Self {
        Self::from_f32(value)
    }
}

impl<const P: u32> Display for Fixed<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&f32::from(*self), f)
    }
}

impl<const P: u32> Debug for Fixed<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, f)
    }
}

#[cfg(test)]
mod test_24_8 {
    use super::*;

    type TestFixed = Fixed<8>;

    const EPS: f32 = TestFixed::EPS.into_f32();

    const SMALL_DATA: &[(f32, f32)] = &[
        (2.5, 1.0),
        (60.0, 20.0),
        (2353.0, 3102.0),
        (3353.0, -1102.0),
        (-200.0, -500.0),
        (-500.0, -200.0),
        (-2.0005, 4.0005),
        (0.0, -34031.0),
        (EPS, EPS),
        (1.0 + EPS, -EPS),
    ];

    const EXTREME_DATA: &[(f32, f32)] = &[
        (2620388.0, 4019.0),
        (2620387.0, 34031.0),
        (1.0, EPS),
        (26.0, EPS),
    ];

    #[test]
    fn addsub() {
        for &(a, b) in SMALL_DATA.iter().chain(EXTREME_DATA.iter()) {
            let ra = f32::from(TestFixed::from(a) + TestFixed::from(b));
            let rs = f32::from(TestFixed::from(a) - TestFixed::from(b));
            let ea = a + b;
            let es = a - b;
            println!("{a} + {b} = {ra}");
            assert!((ra - ea).abs() < EPS);
            println!("{a} - {b} = {rs}");
            assert!((rs - es).abs() < EPS);
        }
    }

    #[test]
    fn mul() {
        for &(a, b) in SMALL_DATA {
            let r = f32::from(TestFixed::from(a) * TestFixed::from(b));
            let e = a * b;
            println!("{a} * {b} = {r}");
            assert!((r - e).abs() < EPS);
        }
    }

    #[test]
    fn div() {
        for &(a, b) in SMALL_DATA.iter().chain(EXTREME_DATA.iter()) {
            let r = f32::from(TestFixed::from(a) / TestFixed::from(b));
            let e = a / b;
            println!("{a} / {b} = {r}");
            assert!((r - e).abs() < EPS);
        }
    }

    const TRUNC_FRACT_DATA: &[(f32, f32, f32)] = &[
        (0.0, 0.0, 0.0),
        (0.5, 0.0, 0.5),
        (1.0, 1.0, 0.0),
        (0.59765625, 0.0, 0.59765625),
        (230.115, 230.0, 0.115),
        (1.0 + 2.0 * EPS, 1.0, 2.0 * EPS),
        (1.0 + EPS, 1.0, EPS),
        (2.0 * EPS, 0.0, 2.0 * EPS),
        (EPS, 0.0, EPS),
    ];

    #[test]
    fn trunc_fract() {
        for (n, w, f) in TRUNC_FRACT_DATA
            .iter()
            .flat_map(|&(n, w, f)| [(n, w, f), (-n, -w, -f)])
            .map(|(a, b, c)| (TestFixed::from_f32(a), b, c))
        {
            let rw = n.trunc().into_f32();
            let rf = n.fract().into_f32();
            println!("{n}.trunc() = {rw}");
            println!("{n}.fract() = {rf}");
            assert!((rw - w).abs() < EPS);
            assert!((rf - f).abs() < EPS);
        }
    }

    const ROUND_DATA: &[f32] = &[0.0, 0.5, 0.6, 0.495, 0.499, 1.0];

    #[test]
    fn round() {
        for &d in ROUND_DATA {
            let ep = d.round();
            let en = (-d).round();
            let rp = TestFixed::from_f32(d).round().into_f32();
            let rn = TestFixed::from_f32(-d).round().into_f32();
            println!("{d}.round() = {rp}");
            println!("{}.round() = {rn}", -d);
            println!("{ep} {en}");
            assert!((ep - rp).abs() < EPS);
            assert!((rn - en).abs() < EPS);
        }
    }
}
