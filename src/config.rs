use log::{debug, warn, LogContext};

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

            $vis fn set_str(&mut self, name: &str, value: &str) -> Result<bool, ::util::AnyError> {
                match name {
                    $($option_name =>
                        self.$field_name = crate::config::define_option_group!(@parse value $(,$parse_fun)?),)*
                    _ => return Ok(false)
                }

                Ok(true)
            }
        }
    };
    (@parse $value: ident) => { crate::config::OptionFromStr::from_str($value)? };
    (@parse $value: ident, $parse_fun: expr) => { $parse_fun($value)? };
}

pub(crate) use define_option_group;

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

            pub fn set_str(&mut self, name: &str, value: &str) -> Result<bool, ::util::AnyError> {
                $(
                if let Some(rest) = name.strip_prefix(concat!($group_name, "-")) {
                    return self.$name.set_str(rest, value);
                }
                )*

                Ok(false)
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
    pub fn must_set_str(&mut self, name: &str, value: &str) -> Result<(), ::util::AnyError> {
        match self.set_str(name, value) {
            Ok(true) => Ok(()),
            Ok(false) => Err("option does not exist".into()),
            Err(err) => Err(err),
        }
    }

    pub fn from_env(log: &LogContext) -> Self {
        let mut result = Config::DEFAULT;

        if let Ok(s) = std::env::var("SBR_CONFIG") {
            for token in s.split(",") {
                let (key, value) = token.split_once("=").unwrap_or((token, "yes"));
                if let Err(err) = result.must_set_str(key, value) {
                    warn!(log, "error setting `{key}={value}` from environment: {err}");
                }
                debug!(log, "set `{key}={value}` from environment")
            }
        }

        result
    }
}
