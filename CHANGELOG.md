## [Unreleased]

- Added a C API for setting a custom logging callback.
- Added options to `xtask install` that allow configuring what type of libraries get installed.
- `xtask install` now outputs some cargo-style status messages.
- `xtask build` and `xtask install` now accept positional arguments that they pass through to `cargo build`.
- Fixed ARM64 Windows implib by updating to implib 0.4. (https://github.com/afishhh/subrandr/issues/31)

[Unreleased]: https://github.com/afishhh/ftlman/compare/v0.1.0...HEAD
