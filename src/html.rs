mod entities;

// https://html.spec.whatwg.org/multipage/parsing.html#character-reference-state
pub fn unescape(text: &str) -> Option<String> {
    let mut result = String::new();

    let mut last = 0;
    let mut i = 0;
    while let Some(n) = text[i..].find('&').map(|n| i + n) {
        macro_rules! flush {
            ($push: expr, $length: expr) => {
                result.push_str(&text[last..n]);
                $push;
                last = n + $length;
                i = last;
                continue;
            };
            () => {{
                i = n + 1;
                continue;
            }};
        }

        let ref_bytes = &text.as_bytes()[n..];
        match ref_bytes.get(1) {
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9') => {
                // Named character reference state
                if let Some((replacement, length)) = entities::consume(&ref_bytes[1..]) {
                    flush!(result.push_str(replacement), 1 + length);
                } else {
                    flush!();
                }
            }
            Some(b'#') => {
                let mut character_reference_code: u32 = 0;
                let mut len;

                // Numeric character reference state
                match ref_bytes.get(2) {
                    Some(b'X' | b'x') => {
                        // Hexadecimal character reference start state
                        len = 3;
                        for &byte in &ref_bytes[3..] {
                            match byte {
                                b'0'..=b'9' => {
                                    len += 1;
                                    character_reference_code = character_reference_code
                                        .saturating_mul(16)
                                        .saturating_add((byte - b'0').into());
                                }
                                b'A'..=b'F' => {
                                    len += 1;
                                    character_reference_code = character_reference_code
                                        .saturating_mul(16)
                                        .saturating_add((byte - 0x37).into());
                                }
                                b'a'..=b'f' => {
                                    len += 1;
                                    character_reference_code = character_reference_code
                                        .saturating_mul(16)
                                        .saturating_add((byte - 0x57).into());
                                }
                                b';' => {
                                    len += 1;
                                    break;
                                }
                                _ => break,
                            }
                        }

                        // Deferred check for absence-of-digits-in-numeric-character-reference parse error
                        if len == 3 {
                            flush!();
                        }
                    }
                    // Decimal character reference start state
                    Some(&digit @ (b'0'..=b'9')) => {
                        character_reference_code = u32::from(digit - b'0');

                        // Decimal character reference state
                        len = 3;
                        for &byte in &ref_bytes[3..] {
                            match byte {
                                b'0'..=b'9' => {
                                    len += 1;
                                    character_reference_code = character_reference_code
                                        .saturating_mul(10)
                                        .saturating_add((byte - b'0').into());
                                }
                                b';' => {
                                    len += 1;
                                    break;
                                }
                                _ => {
                                    break;
                                }
                            }
                        }
                    }
                    _ => flush!(),
                }

                // Numeric character reference end state
                let chr =
                    char::from_u32(character_reference_code).unwrap_or(char::REPLACEMENT_CHARACTER);

                let out = match chr {
                    '\u{0}' => char::REPLACEMENT_CHARACTER,
                    '\u{80}' => '€',
                    '\u{82}' => '‚',
                    '\u{83}' => 'ƒ',
                    '\u{84}' => '„',
                    '\u{85}' => '…',
                    '\u{86}' => '†',
                    '\u{87}' => '‡',
                    '\u{88}' => 'ˆ',
                    '\u{89}' => '‰',
                    '\u{8A}' => 'Š',
                    '\u{8B}' => '‹',
                    '\u{8C}' => 'Œ',
                    '\u{8E}' => 'Ž',
                    '\u{91}' => '‘',
                    '\u{92}' => '’',
                    '\u{93}' => '“',
                    '\u{94}' => '”',
                    '\u{95}' => '•',
                    '\u{96}' => '–',
                    '\u{97}' => '—',
                    '\u{98}' => '˜',
                    '\u{99}' => '™',
                    '\u{9A}' => 'š',
                    '\u{9B}' => '›',
                    '\u{9C}' => 'œ',
                    '\u{9E}' => 'ž',
                    '\u{9F}' => 'Ÿ',
                    other => other,
                };

                flush!(result.push(out), len);
            }
            _ => {
                flush!();
            }
        }
    }

    if result.is_empty() {
        None
    } else {
        result.push_str(&text[last..]);
        Some(result)
    }
}

#[cfg(test)]
mod test {
    use super::unescape;

    #[test]
    fn named_ref_example() {
        assert_eq!(
            unescape("I'm &notit; I tell you").as_deref(),
            Some("I'm ¬it; I tell you")
        );

        assert_eq!(
            unescape("I'm &notin; I tell you").as_deref(),
            Some("I'm ∉ I tell you")
        );
    }

    #[test]
    fn named_ref_more() {
        assert_eq!(
            unescape("I'm &angle;&amp&amp; I tell you").as_deref(),
            Some("I'm ∠&& I tell you")
        );

        assert_eq!(
            unescape("I'm &not&amp&in;&amp I tell you&UpperRightArrow;!").as_deref(),
            Some("I'm ¬&∈& I tell you↗!")
        );

        assert_eq!(
            unescape("&not&invalid;&amp &invalid...").as_deref(),
            Some("¬&invalid;& &invalid...")
        );
    }

    #[test]
    fn hex_ref() {
        assert_eq!(
            unescape("... &#x89&#X96;&#X85 ...").as_deref(),
            Some("... ‰–… ...")
        );

        assert_eq!(
            unescape("... &#x2030;&#X2013&#X2026; ...").as_deref(),
            Some("... ‰–… ...")
        );

        assert_eq!(
            unescape("... &#xfffffffff;, &#Xfffd, &#x00000 ...").as_deref(),
            Some("... �, �, � ...")
        );
    }

    #[test]
    fn dec_ref() {
        assert_eq!(
            unescape("... &#137&#150;&#133 ...").as_deref(),
            Some("... ‰–… ...")
        );

        assert_eq!(
            unescape("... &#8240;&#8211&#8230; ...").as_deref(),
            Some("... ‰–… ...")
        );

        assert_eq!(
            unescape("... &#99999999999;, &#65533, &#0000 ...").as_deref(),
            Some("... �, �, � ...")
        );
    }
}
