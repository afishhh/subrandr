//! Trie data structure for looking up HTML named character references.
//!
//! Compressed to fit entirely in 53040 bytes with hopefully reasonable
//! lookup times. No work is done at runtime to construct the trie, it
//! is embedded entirely in read only memory.
//!
//! # Structure
//!
//! This is how a single node looks in memory:
//! | Byte range          | Name           | Type   |
//! |---------------------|----------------|--------|
//! | `0..1`              | `terminal_len` | `u8`   |
//! | `1..2`              | `next_len`     | `u8`   |
//! | `2..3`              | `next_off`     | `u8`   |
//! | `3..3+terminal_len` | `terminal`     | `str`  |
//! | `next_off..`        | `next`         |        |
//!
//! All `next` structures are aligned to 2 bytes, all addressing is done relative to the base of the
//! static memory where the trie data is placed, the root node is located at address zero.
//!
//! Terminals are the HTML character reference values, for example "&" would be
//! the terminal of the last node of "amp;" and "amp". Nodes with zero-length
//! terminals are considered to have no terminal.
//!
//! There are three distinct types of structures used for next-node lookup:
//! 1. If `next_len & 0x80` is not zero then `next_len ^ 0x80` is the length of a string
//!    present at `next_off+2`. This is a constant that must exactly match the matched-to
//!    string. At `next_off` there is a 16-bit little-endian integer address of the node
//!    past this edge.
//! 2. If `next_len` is `DENSE_TABLE_RANGE` then a table with `DENSE_TABLE_RANGE` 16-bit
//!    little-endian integers is present at `next_off`. The integer at index `i`, if non-zero, is a pointer
//!    to the node past the edge that matches the byte `i + DENSE_TABLE_BASE`.
//! 3. Otherwise `next_len` is the length of a `(u8, u16)` array present at `next_off`.
//!    Each element in this array is an edge that will match only the `u8` byte and
//!    points to the node at `u16`.
//!
//! The transition point for picking the dense representation over the sparse representation was
//! picked completely arbitrarily without much thought put into it.
//!
//! See `generate.py` for code that generates the `trie_little_endian.bin` file.

use std::num::NonZeroU16;

const DENSE_TABLE_RANGE: u8 = 74;
const DENSE_TABLE_BASE: u8 = b'1';

#[repr(C)]
struct NextSparsePair {
    byte: u8,
    target: NonZeroU16,
}

#[repr(C, align(2))]
struct Aligned16<const N: usize>([u8; N]);

const TRIE_DATA: Aligned16<53040> = Aligned16(*include_bytes!("./trie_little_endian.bin"));

unsafe fn traverse(ptr: *const u8, remaining: &[u8]) -> Option<(&'static str, u8)> {
    unsafe {
        let mut child_ptr_le = None;
        let mut child_match_len = 0;

        let terminal_len = ptr.read();
        let next_len = ptr.add(1).read();
        let next_off = ptr.add(2).read();

        let data = ptr.add(next_off.into());
        if let Some(const_len) = next_len.checked_sub(0x80) {
            let const_str = std::slice::from_raw_parts(data.add(2), usize::from(const_len));

            if remaining.starts_with(const_str) {
                child_ptr_le = Some(data.cast::<NonZeroU16>().read());
                child_match_len = const_len;
            }
        } else if next_len == DENSE_TABLE_RANGE {
            if let Some(index) = remaining
                .first()
                .and_then(|byte| byte.checked_sub(DENSE_TABLE_BASE))
                .filter(|&byte| byte < DENSE_TABLE_RANGE)
            {
                child_ptr_le = data.cast::<Option<NonZeroU16>>().add(index.into()).read();
                child_match_len = 1;
            }
        } else if let Some(&chr) = remaining.first() {
            let sparse = std::slice::from_raw_parts(data.cast::<NextSparsePair>(), next_len.into());

            for &NextSparsePair { byte, target } in sparse {
                if chr == byte {
                    child_ptr_le = Some(target);
                    child_match_len = 1;
                    break;
                }
            }
        }

        if let Some(child_ptr_le) = child_ptr_le {
            let child_ptr = child_ptr_le.get().to_le();
            if let Some((terminal, len)) = traverse(
                TRIE_DATA.0.as_ptr().add(child_ptr.into()),
                &remaining.get_unchecked(usize::from(child_match_len)..),
            ) {
                return Some((terminal, len + child_match_len));
            }
        }

        if terminal_len > 0 {
            Some((
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                    ptr.add(3),
                    terminal_len.into(),
                )),
                0,
            ))
        } else {
            None
        }
    }
}

pub fn consume(data: &[u8]) -> Option<(&'static str, usize)> {
    unsafe { traverse(TRIE_DATA.0.as_ptr(), data) }.map(|(terminal, len)| (terminal, len.into()))
}

#[cfg(test)]
mod test {
    use super::consume;

    #[test]
    fn simple() {
        assert_eq!(consume(b"amp;rest"), Some(("&", 4)));
        assert_eq!(consume(b"ampmore"), Some(("&", 3)));
        assert_eq!(consume(b"amp;"), Some(("&", 4)));
        assert_eq!(consume(b"amp"), Some(("&", 3)));
        assert_eq!(consume(b" amp"), None);
        assert_eq!(consume(b"angle;"), Some(("∠", 6)));
        assert_eq!(consume(b"angle"), None);
    }

    #[test]
    fn cent() {
        assert_eq!(consume(b"cent"), Some(("¢", 4)));
    }

    #[test]
    fn lt() {
        assert_eq!(consume(b"lt"), Some(("<", 2)));
        assert_eq!(consume(b"LT"), Some(("<", 2)));
    }

    include!("./all_entities_test.rs");
}
