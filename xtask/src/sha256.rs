use std::{
    fmt::{Debug, Display},
    io::Read,
    ops::Deref,
    str::FromStr,
};

use anyhow::bail;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

// TODO: This could probably be safer with `ascii::Char` at some point
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HexSha256([u8; 64]);

impl HexSha256 {
    pub fn from_digest(digest: &sha2::digest::Output<sha2::Sha256>) -> Self {
        let to_hex_digit = |v: u8| if v < 10 { b'0' + v } else { b'a' - 10 + v };
        let mut output = [0; 64];

        for (idx, value) in digest.iter().enumerate() {
            output[idx * 2] = to_hex_digit(value >> 4);
            output[idx * 2 + 1] = to_hex_digit(value & 0xF);
        }

        Self(output)
    }

    pub fn from_reader(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut hasher = sha2::Sha256::new();
        std::io::copy(reader, &mut hasher)?;
        Ok(Self::from_digest(&hasher.finalize()))
    }

    pub fn as_short_str(&self) -> &str {
        &self.as_str()[..8]
    }

    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl Display for HexSha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <str as Display>::fmt(self.as_str(), f)
    }
}

impl Debug for HexSha256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <str as Debug>::fmt(self.as_str(), f)
    }
}

impl Deref for HexSha256 {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl FromStr for &HexSha256 {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Ok(array) = s.as_bytes().try_into() else {
            bail!("Incorrect length for sha256 digest")
        };

        if let Some(first_invalid) = s
            .bytes()
            .position(|b| !matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            bail!("Invalid character in digest at position {first_invalid}")
        }

        Ok(unsafe { std::mem::transmute::<&[u8; 64], &HexSha256>(array) })
    }
}

impl Serialize for HexSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HexSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        <&str>::deserialize(deserializer)?
            .parse::<&HexSha256>()
            .map_err(serde::de::Error::custom)
            .cloned()
    }
}
