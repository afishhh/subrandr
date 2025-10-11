## subrandr

subrandr is a subtitle rendering library that aims to fill a gap in open-source rendering of non-ASS subtitles accurately.

Currently most subtitle rendering happens in renderers specialized for a particular format into which other subtitles have to be converted. subrandr's goal is to support multiple formats in one library instead, this allows it to conform to format idiosyncrasies while still sharing a lot of code which would otherwise be repeated in specialized renderers[^1].

Note that the library is still under heavy development so beware of bugs and the many unimplemented features.

[^1]: This is particularly useful for SRV3 and WebVTT which effectively require supporting non-trivial CSS inline layout.

### Supported subtitle formats

#### SRV3

YouTube's subtitle format in its XML form.

Most features used by [YTSubConverter](https://github.com/arcusmaximus/YTSubConverter) are supported (with the exception of vertical text). Features outside of this set might not be supported, notably this includes auto-generated subtitles which have quirks that are currently not handled fully correctly (although they look *mostly* fine).

> [!WARNING]
> Since the SRV3 format is entirely undocumented, the format is implemented on a best-effort basis and many features are known to be handled incorrectly/unimplemented.
>
> However subrandr is generally more accurate (for the features it supports) than all implementations I know of that are not browser-based.

The most notable limitations for this format currently are:
- No vertical text[^2] or justification support.
- Segments are laid out as `inline`s instead of `inline-block`s. This mostly manifests in background boxes that are too short vertically.
- Some attributes seen in auto-generated subtitles that seem to vastly change how the HTML tree is generated are unsupported.

[^2]: I have never encountered a YouTube video with subtitles which actually use SRV3 vertical text, so it is not very high-priority.

#### WebVTT

[WebVTT: The Web Video Text Tracks Format](https://www.w3.org/TR/webvtt1/), supported natively by browsers.

Currently subrandr is in the [User agents that do not support CSS](https://www.w3.org/TR/webvtt1/#user-agents-that-do-not-support-css) conformance class of the specification. However note that subrandr does actually implement inline layout as specified by CSS and supports complex layout elements like ruby annotations.

The most notable limitations for this format currently are:
- No vertical text support
- No CSS (`STYLE` block) support
- No region support

### Usage

> [!WARNING]
> Currently the only stable API provided is the C API, **do not** use this library from Rust. Absolutely no API stability is guaranteed on the Rust side.

#### C

The C library can be built and installed with `cargo xtask install` (see `--help` for options). Optionally the build step can also be run separately with `cargo xtask build`.

Definitions and documentation is provided in the `subrandr/*` headers (`include/*` in this repository). Items marked there as unstable require the `SBR_ALLOW_UNSTABLE` macro to be defined. 

> [!WARNING]
> No stability guarantees, neither ABI nor API, are provided for items marked unstable.
> `SBR_ALLOW_UNSTABLE` should only be used when loading at runtime after checking the version or in statically linked builds.

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <subrandr/subrandr.h>

int main() {
  sbr_library *sbr = sbr_library_init();
  if (!sbr)
    // use sbr_get_last_error_string to get a string representation
    // of the last error that occurred in a subrandr function
    exit(1);

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

  sbr_subtitle_context ctx = {
      // this is **dots per inch**, not **pixels per inch**
      // if you have pixels per inch: dpi = ppi * 72 / 96
      .dpi = 144,
      // video dimensions and padding are in 26.6 fixed point format
      .video_width = 1920 << 6,
      .video_height = 1080 << 6,
      // if your player has additional padding around the video (for example
      // black bars)
      // you should provide it here, srv3 subtitles are laid out differently
      // depending
      // on this padding
      .padding_left = 0,
      .padding_right = 0,
      .padding_top = 0,
      .padding_bottom = 0,
  };

  uint32_t t = 2424 /* milliseconds */;

  uint32_t width = 1920;
  uint32_t height = 1080;
  uint32_t *pixel_buffer = malloc(height * width * 4);
  if (!pixel_buffer) {
    printf("malloc failed\n");
    exit(1);
  }

  sbr_renderer_set_subtitles(renderer, subs);
  if (sbr_renderer_render(renderer, &ctx, t, pixel_buffer, width, height,
                          width) < 0)
    exit(1);

  // blit bitmap to the screen OVER the viewport
  // note: bitmap is already premultiplied, use premultiplied blending function

  // some time later

  // you own the bitmap at all times and can free it whenever you want
  free(pixel_buffer);
  sbr_renderer_destroy(renderer);
  sbr_subtitles_destroy(subs);
  // the library must be destroyed only after all associated renderers and
  // subtitles have also been destroyed
  sbr_library_fini(sbr);
}
```

The example code above is licensed under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (public domain).

#### WebAssembly library and extension

There's a WebAssembly library being worked on in the `wasi32` directory and a webextension that is able to successfully render subtitles over YouTube videos on youtube.com in the `webextension` directory.

Currently it is still a work-in-progress and there's a few difficult problems that still have to be solved. Although it does work, keep in mind that performance is not that great (especially on HiDPI displays) and no API stability is guaranteed.

#### `sbr-overlay`

The `sbr-overlay` directory contains a test program that can be used to test the library in software rasterization mode on X11 and with wgpu on other platforms. It renders subtitles onto a transparent window and also provides some additional functionality to synchronize with and overlay on top of existing video players. 

Subtitles can be synchronized with video playback in [mpv](https://github.com/mpv-player/mpv/) by specifying `--connect mpv:<PATH_TO_MPV_SOCKET>` (with a path to a socket created using the `--input-ipc-server` mpv option).

~~It is also possible to overlay over a YouTube tab using `--connect youtube-cdp:<CDP_URL>` which lets you specify the URL to a Chrome DevTools Protocol WebSocket, it will automatically attach to the first YouTube tab it finds and collect information from it to (mostly) correctly overlay and synchronize the subtitles.~~
[This is currently broken](https://github.com/afishhh/subrandr/issues/47) and likely to be removed soon&trade;.

The mpv IPC implementation is able to automatically acquire the X11 window id of the mpv window, in other cases you need to specify the window id via the `--overlay` option to have the window overlaid on the target (also only supported on X11).

### Hardware acceleration

subrandr currently supports an alpha-quality hardware-accelerated wgpu rasterizer. It is still experimental and provides zero or negative performance gains.

There are quite a few blocking issues that need to be resolved before working on the wgpu rasterizer makes sense again. Due to these issues and more it is not exposed in either the C API or the WASM module yet.

### License

subrandr is available under the Mozilla Public License 2.0, see [LICENSE](https://github.com/afishhh/subrandr/blob/master/LICENSE) for details. Unless stated otherwise all files in this repository are covered by this license.
