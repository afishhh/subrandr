use text_sys::fontconfig::*;
use util::math::I16Dot16;

// https://gitlab.freedesktop.org/fontconfig/fontconfig/-/blob/main/src/fcweight.c
#[rustfmt::skip]
const WEIGHT_MAP: &[(I16Dot16, I16Dot16)] = &[
    (I16Dot16::new(FC_WEIGHT_THIN as i32), I16Dot16::new(100)),
    (I16Dot16::new(FC_WEIGHT_EXTRALIGHT as i32), I16Dot16::new(200)),
    (I16Dot16::new(FC_WEIGHT_LIGHT as i32), I16Dot16::new(300)),
    (I16Dot16::new(FC_WEIGHT_DEMILIGHT as i32), I16Dot16::new(350)),
    (I16Dot16::new(FC_WEIGHT_BOOK as i32), I16Dot16::new(380)),
    (I16Dot16::new(FC_WEIGHT_REGULAR as i32), I16Dot16::new(400)),
    (I16Dot16::new(FC_WEIGHT_MEDIUM as i32), I16Dot16::new(500)),
    (I16Dot16::new(FC_WEIGHT_SEMIBOLD as i32), I16Dot16::new(600)),
    (I16Dot16::new(FC_WEIGHT_BOLD as i32), I16Dot16::new(700)),
    (I16Dot16::new(FC_WEIGHT_EXTRABOLD as i32), I16Dot16::new(800)),
    (I16Dot16::new(FC_WEIGHT_BLACK as i32), I16Dot16::new(900)),
    (I16Dot16::new(FC_WEIGHT_EXTRABLACK as i32), I16Dot16::new(1000)),
];

pub fn map_fontconfig_weight_to_opentype(fc_weight: I16Dot16) -> Option<I16Dot16> {
    if fc_weight < 0 || fc_weight > I16Dot16::new(FC_WEIGHT_EXTRABLACK as i32) {
        return None;
    }

    let i = WEIGHT_MAP
        .binary_search_by(|x| x.0.partial_cmp(&fc_weight).unwrap())
        .map_or_else(std::convert::identity, std::convert::identity);

    if WEIGHT_MAP[i].0 == fc_weight {
        return Some(WEIGHT_MAP[i].1);
    }

    Some({
        let fc_start = WEIGHT_MAP[i - 1].0;
        let fc_end = WEIGHT_MAP[i].0;
        let ot_start = WEIGHT_MAP[i - 1].1;
        let ot_end = WEIGHT_MAP[i].1;
        let fc_diff = fc_end - fc_start;
        let ot_diff = ot_end - ot_start;
        ot_start + (fc_weight - fc_start) * ot_diff / fc_diff
    })
}

pub fn map_opentype_weight_to_fontconfig(ot_weight: I16Dot16) -> Option<I16Dot16> {
    if !(I16Dot16::new(0)..=I16Dot16::new(1000)).contains(&ot_weight) {
        return None;
    }

    if ot_weight <= 100 {
        return Some(I16Dot16::new(FC_WEIGHT_THIN as i32));
    }

    let i = WEIGHT_MAP
        .binary_search_by(|x| x.1.partial_cmp(&ot_weight).unwrap())
        .map_or_else(std::convert::identity, std::convert::identity);

    if WEIGHT_MAP[i].1 == ot_weight {
        return Some(WEIGHT_MAP[i].0);
    }

    Some({
        let fc_start = WEIGHT_MAP[i - 1].0;
        let fc_end = WEIGHT_MAP[i].0;
        let ot_start = WEIGHT_MAP[i - 1].1;
        let ot_end = WEIGHT_MAP[i].1;
        let fc_diff = fc_end - fc_start;
        let ot_diff = ot_end - ot_start;
        fc_start + (ot_weight - ot_start) * fc_diff / ot_diff
    })
}

#[cfg(test)]
mod test {
    use std::ops::Range;

    use super::*;

    #[test]
    fn fontconfig_to_opentype_weight_mapping() {
        for &(fc, ot) in &WEIGHT_MAP[1..] {
            assert_eq!(map_fontconfig_weight_to_opentype(fc), Some(ot));
        }

        assert_eq!(map_fontconfig_weight_to_opentype(I16Dot16::new(-1)), None);
        assert_eq!(map_fontconfig_weight_to_opentype(I16Dot16::new(300)), None);

        const LERP_CASES: &[(i32, Range<i32>)] = &[
            (30, 100..200),
            (60, 350..380),
            (213, 900..1000),
            (203, 700..800),
        ];

        for (fc, ot_range) in LERP_CASES.iter().map(|&(fc, Range { start, end })| {
            (I16Dot16::new(fc), I16Dot16::new(start)..I16Dot16::new(end))
        }) {
            println!("mapping {fc} to opentype, expecting a result in {ot_range:?}");
            let result = map_fontconfig_weight_to_opentype(fc).unwrap();
            println!("got: {result}");
            assert!(ot_range.contains(&result));
        }
    }

    #[test]
    fn opentype_to_fontconfig_weight_mapping() {
        for &(fc, ot) in WEIGHT_MAP {
            assert_eq!(map_opentype_weight_to_fontconfig(ot), Some(fc));
        }

        assert_eq!(map_opentype_weight_to_fontconfig(I16Dot16::new(-1)), None);
        assert_eq!(map_opentype_weight_to_fontconfig(I16Dot16::new(1100)), None);

        const LERP_CASES: &[(i32, Range<i32>)] = &[
            (150, FC_WEIGHT_THIN as i32..FC_WEIGHT_EXTRALIGHT as i32),
            (250, FC_WEIGHT_EXTRALIGHT as i32..FC_WEIGHT_LIGHT as i32),
            (375, FC_WEIGHT_DEMILIGHT as i32..FC_WEIGHT_BOOK as i32),
            (750, FC_WEIGHT_BOLD as i32..FC_WEIGHT_EXTRABOLD as i32),
            (950, FC_WEIGHT_BLACK as i32..FC_WEIGHT_EXTRABLACK as i32),
        ];

        for (fc, ot_range) in LERP_CASES.iter().map(|&(fc, Range { start, end })| {
            (I16Dot16::new(fc), I16Dot16::new(start)..I16Dot16::new(end))
        }) {
            println!("mapping {fc} to fontconfig, expecting a result in {ot_range:?}");
            let result = map_opentype_weight_to_fontconfig(fc).unwrap();
            println!("got: {result}");
            assert!(ot_range.contains(&result));
        }
    }
}
