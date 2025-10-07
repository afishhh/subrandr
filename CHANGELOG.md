## [Unreleased]

- Fixed newline-only srv3 events creating empty visual lines (they are now ignored).
- Fixed incorrect advance being used during layout of glyphs from non-scalable fonts. Mostly affects emoji which previously took up excessive horizontal space.
- Implemented srv3 visual line padding.
- Switched `xtask (build|install)` to use `cargo rustc` and made it stop building disabled library types.
- Made `xtask (build|install)` query rustc for required static libraries instead of hard-coding them.
- Fixed ruby annotation layout on non-72 DPIs.
- Improved text decorations to better conform to the `css-2` specification. WebVTT underlines will now be painted after shadows and before the text itself.
- Fixed incorrect fractional positioning of blurred shadows. This fixes blurred shadows *slightly* jumping around in karaoke subtitles.

## [v0.2.2]

- Fixed out-of-bounds write in software rasterizer.
  (This is what releasing 3AM code gets me)

## [v0.2.1]

- *Significantly* improved software rasterization performance.
- Fixed pixel scale handling in font matching. After the introduction of the new inline layout engine fonts were being unintentionally scaled *twice* on DPIs other than 72.
- Added Android NDK font provider for better font matching on Android.

[Unreleased]: https://github.com/afishhh/subrandr/compare/v0.2.2...HEAD
[v0.2.2]: https://github.com/afishhh/subrandr/compare/v0.2.1...v0.2.2
[v0.2.1]: https://github.com/afishhh/subrandr/compare/v0.2.0...v0.2.1
