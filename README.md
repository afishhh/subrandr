## subrandr

subrandr is a subtitle renderer for the SRV3 and WebVTT subtitle formats.

## Supported subtitle formats

### SRV3

YouTube's subtitle format in its XML form.

Most features used by [YTSubConverter](https://github.com/arcusmaximus/YTSubConverter) are supported. Some additional features found in auto-generated subtitles are also supported.

> [!WARNING]
> Since the SRV3 format is entirely undocumented, it's implemented on a best-effort basis and many features are known to be handled incorrectly/unimplemented.
>
> However, subrandr is generally more accurate than all other open implementations I know of.

The most notable limitations for this format currently are:
- Partial vertical text support
- No justification support

### WebVTT

[WebVTT: The Web Video Text Tracks Format](https://www.w3.org/TR/webvtt1/), supported natively by browsers.

Currently subrandr is in the [User agents that do not support CSS](https://www.w3.org/TR/webvtt1/#user-agents-that-do-not-support-css) conformance class of the specification. This is because it does not implement the full feature set technically required by the [User agents that do not support a full HTML CSS engine](https://www.w3.org/TR/webvtt1/#user-agents-that-do-not-support-a-full-html-css-engine) conformance class but it does implement a sizable chunk like parsing of CSS stylesheets and proper CSS inline layout.

The most notable limitations for this format currently are:
- No vertical text support
- Incomplete CSS selector/property support
- No region support

## Requirements

subrandr unconditionally depends on the following non-Rust libraries:
- FreeType2
- HarfBuzz

The following libraries/APIs are used for system font providers:
- (*on Unix platforms except Android*) Fontconfig
- (*on Android*) Android NDK (API level >=29)
- (*on Windows*) DirectWrite

Font providers can be enabled/disabled via cargo feature flags of the main crate. If the (enabled by default) `platform-defaults` feature is enabled, font providers are automatically enabled based on the condition preceding it in parenthesis in the above list. To manually decide which font providers to build, disable default features and enable specific `font-provider-<name>` features.

## Usage

> [!WARNING]
> Currently the only stable API provided is the C API. The Rust API of the core and internal crates is permanently unstable and breaking changes happen often.

### C

The C library can be built and installed with `cargo xtask install` (see `--help` for options). Optionally, the build step can also be run separately with `cargo xtask build`.

Definitions are provided in the `<subrandr/*>` headers (`include/*` in this repository) along with documentation in the form of doc comments.

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <subrandr/subrandr.h>

int main() {
  sbr_library *sbr = sbr_library_init();
  if (!sbr) {
    // Use `sbr_get_last_error_string` to get a string representation of the
    // last error that occurred in a subrandr function on this thread.
    exit(1);
  }

  char *content = "<timedtext format=\"3\"><head></head>"
                  "<body><p t=\"0\" d=\"5000\">Hello, world!</p></body>"
                  "</timedtext>";
  sbr_subtitles *subs = sbr_load_text(sbr, content, strlen(content),
                                      SBR_SUBTITLE_FORMAT_UNKNOWN, NULL);
  if (!subs)
    exit(1);

  sbr_renderer *renderer = sbr_renderer_create(sbr);
  if (!renderer)
    exit(1);

  uint32_t width = 1920;
  uint32_t height = 1080;
  uint32_t *pixel_buffer = calloc(height * width * 4);
  if (!pixel_buffer) {
    printf("malloc failed\n");
    exit(1);
  }

  uint32_t t = 2424 /* milliseconds */;
  sbr_subtitle_context ctx = {
      .dpi = 144,
      // Video dimensions and padding are in 26.6 fixed point format.
      .video_width = 1920 << 6,
      .video_height = 1080 << 6,
      // If your player has additional padding around the video (black bars),
      // you can provide it here.
      .padding_left = 0,
      .padding_right = 0,
      .padding_top = 0,
      .padding_bottom = 0,
  };

  sbr_renderer_set_subtitles(renderer, subs);
  // Alternatively: consider `sbr_renderer_render_instanced` for better performance.
  if (sbr_renderer_render(renderer, &ctx, t, pixel_buffer, width, height, width) < 0)
    exit(1);

  // `bitmap` now contains premultiplied sRGB in BGRA format that can be blended
  // over the viewport.
  // Due to legacy web reasons, this is expected to be done in gamma-encoded sRGB
  // and *not* linear light.

  free(pixel_buffer);
  sbr_renderer_destroy(renderer);
  sbr_subtitles_destroy(subs);
  // The library must be destroyed only after all associated objects have
  // been destroyed.
  sbr_library_fini(sbr);
}
```

The example code above is licensed under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (public domain).

#### WebAssembly library and extension

There's a WebAssembly library being worked on in the `wasi32` directory and a webextension that is able to successfully render subtitles over YouTube videos on youtube.com in the `webextension` directory.

Currently it is still a work-in-progress and there's a few difficult problems that still have to be solved. Although it does work, keep in mind that performance is not great (especially on HiDPI displays) and no API stability is guaranteed.

#### `sbr-overlay`

The `sbr-overlay` directory contains a test program that can be used to test the library in software rasterization mode on X11. It renders subtitles onto a transparent window and also provides some additional functionality to synchronize with and overlay on top of existing video players. 

Subtitles can be synchronized with video playback in [mpv](https://github.com/mpv-player/mpv/) by specifying `--connect mpv:<PATH_TO_MPV_SOCKET>` (with a path to a socket created using the `--input-ipc-server` mpv option).

~~It is also possible to overlay over a YouTube tab using `--connect youtube-cdp:<CDP_URL>` which lets you specify the URL to a Chrome DevTools Protocol WebSocket, it will automatically attach to the first YouTube tab it finds and collect information from it to (mostly) correctly overlay and synchronize the subtitles.~~
[This is currently broken](https://github.com/afishhh/subrandr/issues/47) and likely to be removed soon&trade;.

The mpv IPC implementation is able to automatically acquire the X11 window id of the mpv window, in other cases you need to specify the window id via the `--overlay` option to have the window overlaid on the target (also only supported on X11).

## License

subrandr is available under the Mozilla Public License 2.0, see [LICENSE](https://github.com/afishhh/subrandr/blob/master/LICENSE) for details. Unless stated otherwise all files in this repository are covered by this license.
