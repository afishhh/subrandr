use log::{debug, warn, LogContext};
use thiserror::Error;

pub(crate) trait OptionFromStr: Sized {
    fn from_str(value: &str) -> Result<Self, util::AnyError>;
}

impl OptionFromStr for bool {
    fn from_str(value: &str) -> Result<Self, util::AnyError> {
        Ok(match value {
            "yes" => true,
            "no" => false,
            _ => return Err("must be either \"yes\" or \"no\"".into()),
        })
    }
}

macro_rules! impl_from_str_parser {
    ($($type: ty),*) => {
        $(impl OptionFromStr for $type {
            fn from_str(s: &str) -> Result<Self, util::AnyError> {
                <$type as std::str::FromStr>::from_str(s).map_err(Into::into)
            }
        })*
    };
}

impl_from_str_parser!(u8, u16, u32, u64, u128);
impl_from_str_parser!(i8, i16, i32, i64, i128);

impl OptionFromStr for BGRA8 {
    fn from_str(s: &str) -> Result<Self, util::AnyError> {
        const ERROR: &str = "must be a color in #RRGGBB(AA) form";

        let hex = s.strip_prefix("#").ok_or(ERROR)?;
        if hex.len() != 6 && hex.len() != 8 {
            return Err(ERROR.into());
        }

        let mut value = u32::from_str_radix(hex, 16)?;
        if hex.len() == 6 {
            value <<= 8;
            value |= 0xFF;
        }
        Ok(Self::from_rgba32(value))
    }
}

#[derive(Debug, Error)]
pub enum SetStrError {
    #[error("option not found")]
    NotFound,
    #[error(transparent)]
    InvalidValue(#[from] util::AnyError),
}

macro_rules! define_option_group {
    ($vis: vis struct $name: ident {
        $(
            #[option(name = $option_name: literal $(, parse_with = $parse_fun: expr)?)]
            $field_vis: vis $field_name: ident: $field_ty: ty = $default: expr,
        )*
    }) => {
        #[derive(Debug, Clone)]
        $vis struct $name {
            $($field_vis $field_name: $field_ty,)*
        }

        impl $name {
            $vis const DEFAULT: Self = Self {
                $($field_name: $default,)*
            };

            $vis fn set_str(&mut self, name: &str, value: &str) -> Result<(), crate::config::SetStrError> {
                match name {
                    $($option_name =>
                        self.$field_name = crate::config::define_option_group!(@parse value $(,$parse_fun)?),)*
                    _ => return Err(crate::config::SetStrError::NotFound)
                }

                Ok(())
            }
        }
    };
    (@parse $value: ident) => { crate::config::OptionFromStr::from_str($value)? };
    (@parse $value: ident, $parse_fun: expr) => { $parse_fun($value)? };
}

pub(crate) use define_option_group;
use rasterize::color::BGRA8;

macro_rules! define_config_struct {
    (pub struct Config {$(
        #[option(name = $group_name: literal, substruct)]
        pub $name: ident: $substruct: ty
    ),*}) => {
        #[derive(Debug, Clone)]
        pub struct Config {
            $(pub(crate) $name: $substruct,)*
        }

        impl Config {
            pub const DEFAULT: Self = Self {
                $($name: <$substruct>::DEFAULT,)*
            };

            pub fn set_str(&mut self, name: &str, value: &str) -> Result<(), crate::config::SetStrError> {
                $(
                if let Some(rest) = name.strip_prefix(concat!($group_name, "-")) {
                    return self.$name.set_str(rest, value);
                }
                )*

                Err(crate::config::SetStrError::NotFound)
            }
        }
    };
}

define_config_struct! {
    pub struct Config {
        #[option(name = "srv3", substruct)]
        pub srv3: crate::srv3::Options,
        #[option(name = "debug", substruct)]
        pub debug: crate::renderer::DebugOptions
    }
}

impl Config {
    pub fn from_env(log: &LogContext) -> Self {
        let mut result = Config::DEFAULT;

        if let Ok(s) = std::env::var("SBR_CONFIG") {
            for token in s.split(",") {
                let (key, value) = token.split_once("=").unwrap_or((token, "yes"));
                if let Err(err) = result.set_str(key, value) {
                    warn!(log, "error setting `{key}={value}` from environment: {err}");
                }
                debug!(log, "set `{key}={value}` from environment")
            }
        }

        result
    }
}
