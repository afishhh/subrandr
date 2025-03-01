use std::{
    fmt::{Debug, Display},
    ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign},
};

use text_sys::FT_Fixed;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed<const P: u32, T>(T);

macro_rules! define_simple_fixed_operator {
            ($ftype: ty, @all $($tt: tt)*) => {
                define_simple_fixed_operator!(
                    $ftype,
                    [Self [Self] [.0]],
                    $($tt)*
                );
                define_simple_fixed_operator!(
                    $ftype,
                    @conversions
                    $($tt)*
                );
            };
            ($ftype: ty, @conversions $($tt: tt)*) => {
                define_simple_fixed_operator!(
                    $ftype,
                    [$ftype [] [] Self::new],
                    $($tt)*
                );
                define_simple_fixed_operator!(
                    $ftype,
                    [f32 [] [] Self::from_f32],
                    $($tt)*
                );
            };
            (
                $ftype: ty,
                [$type: ty [$($ctor: tt)?] [$($dot: tt)*] $($construct: tt)*],
                $trait: ident,
                $f: ident,
                $op: tt,
                $trait_assign: ident,
                $f_assign: ident,
                $op_assign: tt
            ) => {
                impl<const P: u32> $trait<$type> for Fixed<P, $ftype> {
                    type Output = Self;

                    fn $f(self, rhs: $type) -> Self::Output {
                        $($ctor)? (self$($dot)* $op $($construct)*(rhs)$($dot)*)
                    }
                }

                impl<const P: u32> $trait_assign<$type> for Fixed<P, $ftype> {
                    fn $f_assign(&mut self, rhs: $type) {
                        (*self)$($dot)* $op_assign $($construct)*(rhs)$($dot)*
                    }
                }
            };
        }

// TODO: Once a way to have const functions in traits is stabilised
//       rewrite this to use a trait instead, as that will allow for
//       better inference and thus ergonomics.
macro_rules! define_fixed_for_type {
    (
        signedness = $signedness: tt,
        inner = $type: ty,
        widen = $wide: ty
        $(, unsigned = $unsigned: ty)?
    ) => {
        impl<const P: u32> Fixed<P, $type> {
            pub const fn new(value: $type) -> Self {
                Self(value << P)
            }

            pub const fn from_quotient(numerator: $type, denominator: $type) -> Self {
                Self::from_wide_quotient(numerator as $wide, denominator as $wide)
            }

            pub const fn from_wide_quotient(numerator: $wide, denominator: $wide) -> Self {
                Self(((numerator << P) / denominator) as $type)
            }

            pub const fn from_raw(value: $type) -> Self {
                Self(value)
            }

            pub const fn into_raw(self) -> $type {
                self.0
            }

            pub const fn from_f32(value: f32) -> Self {
                Self((value * (1 << P) as f32) as $type)
            }

            pub const fn into_f32(self) -> f32 {
                self.0 as f32 / (1 << P) as f32
            }

            pub const fn floor(self) -> Self {
                Self((self.0 >> P) << P)
            }

            pub const fn floor_to_inner(self) -> $type {
                self.0 >> P
            }

            pub const fn trunc_to_inner(self) -> $type {
                self.trunc().0 >> P
            }

            pub const fn round_to_inner(self) -> $type {
                self.round().0 >> P
            }

            pub fn ceil(self) -> Self {
                Self(self.0 + (Self::ONE.0 - 1)).floor()
            }

            pub fn ceil_to_inner(self) -> $type {
                self.ceil().0 >> P
            }

            pub const ONE: Self = Self(1 << P);
            pub const ZERO: Self = Self(0);
            pub const MIN: Self = Self(<$type>::MIN);
            pub const MAX: Self = Self(<$type>::MAX);
            pub const HALF: Self = Self(1 << (P - 1));
            const EPS: Self = Self(1);

            const FRACTIONAL_MASK: $type = (1 << P) - 1;
            const WHOLE_MASK: $type = !Self::FRACTIONAL_MASK;
        }

        define_fixed_for_type!(@$signedness $type, $wide $(, $unsigned)?);

        impl<const P: u32> Mul for Fixed<P, $type> {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self::Output {
                Self(((self.0 as $wide * rhs.0 as $wide) >> P) as $type)
            }
        }

        impl<const P: u32> MulAssign for Fixed<P, $type> {
            fn mul_assign(&mut self, rhs: Self) {
                *self = *self * rhs;
            }
        }

        impl<const P: u32> Div for Fixed<P, $type> {
            type Output = Self;

            fn div(self, rhs: Self) -> Self::Output {
                let wide_result = ((self.0 as $wide) << P) / rhs.0 as $wide;
                Self(wide_result as $type)
            }
        }

        impl<const P: u32> DivAssign for Fixed<P, $type> {
            fn div_assign(&mut self, rhs: Self) {
                *self = *self / rhs;
            }
        }

        impl<const P: u32> PartialEq<$type> for Fixed<P, $type> {
            fn eq(&self, other: &$type) -> bool {
                (self.0 & Self::FRACTIONAL_MASK) == 0 && (self.0 >> P) == *other
            }
        }

        impl<const P: u32> From<Fixed<P, $type>> for f32 {
            fn from(value: Fixed<P, $type>) -> Self {
                value.into_f32()
            }
        }

        impl<const P: u32> From<f32> for Fixed<P, $type> {
            fn from(value: f32) -> Self {
                Self::from_f32(value)
            }
        }

        impl<const P: u32> Display for Fixed<P, $type> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Display::fmt(&f32::from(*self), f)
            }
        }

        impl<const P: u32> Debug for Fixed<P, $type> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Display::fmt(self, f)
            }
        }

        define_simple_fixed_operator!(
            $type, @all Add, add, +, AddAssign, add_assign, +=
        );

        define_simple_fixed_operator!(
            $type, @all Sub, sub, -, SubAssign, sub_assign, -=
        );

        // TODO: Both Div and Mul can be implemented more efficiently when
        //       multiplying by integers
        define_simple_fixed_operator!(
            $type, @conversions Div, div, /, DivAssign, div_assign, /=
        );

        define_simple_fixed_operator!(
            $type, @conversions Mul, mul, *, MulAssign, mul_assign, *=
        );

        impl<const P: u32> PartialOrd<$type> for Fixed<P, $type> {
            // TODO: Full precision
            fn partial_cmp(&self, other: &$type) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(&(other << P))
            }
        }
    };
    (@signed $type: ty, $wide: ty, $unsigned: ty) => {
        impl<const P: u32> Fixed<P, $type> {
            const SIGN_MASK: $type = 1 << (<$type>::BITS - 1);

            pub const fn trunc(self) -> Self {
                let signed_floor = (self.0 >> P) << P;
                let was_negative_with_fract = (self.0 & (Self::FRACTIONAL_MASK | Self::SIGN_MASK))
                    as u32
                    > Self::SIGN_MASK as u32;
                Self(signed_floor + Self::ONE.0 * (was_negative_with_fract as $type))
            }

            pub const fn fract(self) -> Self {
                let unsigned = self.0 & Self::FRACTIONAL_MASK;
                let mut extension = (self.0 & Self::SIGN_MASK) >> (<$type>::BITS - P - 1);
                extension *= (unsigned > 0) as $type;
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

            pub const fn signum(self) -> Self {
                Self(Self::ONE.0 * (1 | (self.0 >> (<$type>::BITS - 1))))
            }

            pub const fn abs(self) -> Self {
                Self(self.0.abs())
            }

            pub const fn unsigned_abs(self) -> Fixed<P, $unsigned> {
                Fixed(self.0.unsigned_abs())
            }
        }

        impl<const P: u32> Neg for Fixed<P, $type> {
            type Output = Self;

            fn neg(self) -> Self::Output {
                Self(-self.0)
            }
        }

        impl<const P: u32> super::Number for Fixed<P, $type> {
            const ZERO: Self = Self::ZERO;
            const MIN: Self = Self::MIN;
            const MAX: Self = Self::MAX;
        }
    };
    (@unsigned $type: ty, $wide: ty) => {
        impl<const P: u32> Fixed<P, $type> {
            pub const fn trunc(self) -> Self {
                Self((self.0 >> P) << P)
            }

            pub const fn fract(self) -> Self {
                Self(self.0 & Self::FRACTIONAL_MASK)
            }

            pub const fn round(self) -> Self {
                let fract = self.fract();
                if fract.0 >= Self::HALF.0 {
                    Self((self.0 & Self::WHOLE_MASK) + Self::ONE.0)
                }  else {
                    self.trunc()
                }
            }
        }
    };
}

define_fixed_for_type!(
    signedness = signed,
    inner = i64,
    widen = i128,
    unsigned = u64
);
define_fixed_for_type!(
    signedness = signed,
    inner = i32,
    widen = i64,
    unsigned = u32
);
define_fixed_for_type!(signedness = unsigned, inner = u32, widen = u64);
define_fixed_for_type!(signedness = unsigned, inner = u16, widen = u32);

pub type I32Fixed<const P: u32> = Fixed<P, i32>;
pub type U32Fixed<const P: u32> = Fixed<P, u32>;

pub type I26Dot6 = Fixed<6, i32>;
pub type I16Dot16 = Fixed<16, i32>;

impl<const P: u32> I32Fixed<P> {
    #[allow(clippy::unnecessary_cast)]
    pub fn into_ft(self) -> FT_Fixed {
        Self::into_raw(self) as FT_Fixed
    }

    #[allow(clippy::unnecessary_cast)]
    pub fn from_ft(value: FT_Fixed) -> Self {
        Self::from_raw(value as i32)
    }
}

#[cfg(test)]
macro_rules! test_module {
    ($fixed_type: ty, signed = $signed: tt) => {
        use super::*;

        type TestFixed = $fixed_type;

        const EPS: f32 = TestFixed::EPS.into_f32();

        const SMALL_DATA: &[(f32, f32)] = &[
            (2.5, 1.0),
            (60.0, 20.0),
            (2353.0, 3102.0),
            (EPS, EPS),
            (1.0 + EPS, EPS),
            (1.0 - EPS, EPS),
        ];

        const SIGNED_DATA: &[(f32, f32)] = &[
            (3353.0, -1102.0),
            (-200.0, -500.0),
            (-500.0, -200.0),
            (-2.0005, 4.0005),
            (0.0, -34031.0),
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
            for &(a, b) in SMALL_DATA.iter().chain(EXTREME_DATA.iter()).chain(
                const {
                    if $signed {
                        SIGNED_DATA
                    } else {
                        &[]
                    }
                },
            ) {
                let ra = f32::from(TestFixed::from(a) + TestFixed::from(b));
                let ea = a + b;
                println!("{a} + {b} = {ra}");
                assert!((ra - ea).abs() < EPS);
                if $signed {
                    let rs = f32::from(TestFixed::from(a) - TestFixed::from(b));
                    let es = a - b;
                    println!("{a} - {b} = {rs}");
                    assert!((rs - es).abs() < EPS);
                }
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

        const ROUND_DATA: &[f32] = &[
            0.0, 0.5, 0.6, 0.495, 0.499, 1.0, 5.3,
            5.7, 5.5, 5.0, 100.2, 300., 0.59765625,
            230.115, 1.0 + 2.0 * EPS, 1.0 + EPS,
            2.0 * EPS, EPS,
        ];

        #[test]
        fn trunc_fract() {
            for &n in ROUND_DATA {
                let rw = TestFixed::from_f32(n).trunc().into_f32();
                let rf = TestFixed::from_f32(n).fract().into_f32();
                println!("{n}.trunc() = {rw}");
                println!("{n}.fract() = {rf}");
                assert!((rw - n.trunc()).abs() < EPS);
                assert!((rf - n.fract()).abs() < EPS);
            }
        }

        #[test]
        fn floor() {
            for &d in ROUND_DATA {
                let ep = d.floor();
                let rp = TestFixed::from_f32(d).floor().into_f32();
                println!("{d}.floor() = {rp}");
                assert!((ep - rp).abs() < EPS);
                if $signed {
                    let en = (-d).floor();
                    let rn = TestFixed::from_f32(-d).floor().into_f32();
                    println!("{}.floor() = {rn}", -d);
                    assert!((rn - en).abs() < EPS);
                }
            }
        }

        #[test]
        fn round() {
            for &d in ROUND_DATA {
                let ep = d.round();
                let rp = TestFixed::from_f32(d).round().into_f32();
                println!("{d}.round() = {rp}");
                assert!((ep - rp).abs() < EPS);
                if $signed {
                    let en = (-d).round();
                    let rn = TestFixed::from_f32(-d).round().into_f32();
                    println!("{}.round() = {rn}", -d);
                    assert!((rn - en).abs() < EPS);
                }
            }
        }

        #[test]
        fn ceil() {
            for &d in ROUND_DATA {
                let ep = d.ceil();
                let rp = TestFixed::from_f32(d).ceil().into_f32();
                println!("{d}.ceil() = {rp}");
                assert!((ep - rp).abs() < EPS);
                if $signed {
                    let en = (-d).ceil();
                    let rn = TestFixed::from_f32(-d).ceil().into_f32();
                    println!("{}.ceil() = {rn}", -d);
                    assert!((rn - en).abs() < EPS);
                }
            }
        }

        test_module!(@signedness_specific $signed);
    };
    (@signedness_specific false) => {};
    (@signedness_specific true) => {
        #[test]
        fn signum() {
            for a in const { SIGNED_DATA }.iter().flat_map(|&(a, b)| [a, b]) {
                let ra = f32::from(TestFixed::from(a).signum());
                let ea = a.signum();
                println!("{a}.signum() = {ra}");
                assert!((ra - ea).abs() < EPS);
            }
        }
    };
}

#[cfg(test)]
mod test_signed_24_dot_8 {
    test_module!(I32Fixed<8>, signed = true);
}

#[cfg(test)]
mod test_unsigned_24_dot_8 {
    test_module!(U32Fixed<8>, signed = false);
}
