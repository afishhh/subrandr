## subrandr

### An experimental subtitle rendering library

#### Supported subtitle formats:
- ASS (very incomplete, **currently disabled and on hold**)
- SRV3 (less incomplete, still very incomplete)

TODO: ttml is what netflix uses, probably not too difficult to support
TODO: experiment with using fixed point arithmetic for linear RGBA calculations
      and gaussian blur

#### `sbr-overlay`

An example program is also present in this repository. It allows testing this library by rendering subtitles onto a transparent window. To aid in this testing it also provides functionality that allows you to overlay the window on an existing video player (or even the YouTube website).

Subtitles can be synchronized with video playback in the mpv video player by specifying `--connect mpv:<PATH_TO_MPV_SOCKET>` (created using `--input-ipc-server` mpv option). You can also synchronize and overlay over a YouTube tab using `--connect youtube-cdp:<CDP_URL>` which lets you specify the URL to a Chrome DevTools Protocol WebSocket, it will automatically attach to the first YouTube tab it finds and collect information from it to (mostly) correctly overlay and synchronize the subtitles.

The mpv IPC implementation is able to automatically acquire the X11 window id of the mpv window, in other cases you need to specify the window id via the `--overlay` option to have the window overlaid on the target.

#### Usage

Currently only a C API is provided, **do not** use this library from Rust. Absolutely no API stability is guaranteed on the Rust side.
The C API is defined in the `subrandr.h` header, items marked there as unstable require the `SBR_ALLOW_UNSTABLE` macro to be defined and no API stability is guaranteed for them.

> [!WARNING]
> This library is experimental and no API stability is guaranteed for any of its APIs
> even for items not marked unstable. This *may* or *may not* change in the future.

The library performs all rendering on the CPU and renders to a BGRA8888 bitmap the size of your viewport.

```c
#include <stdio.h>
#include <stdlib.h>

#include "subrandr.h"

int main() {
  sbr_library *sbr = sbr_library_init();
  if(!sbr)
  // use sbr_get_last_error_string to get a string representation
  // of the last error that occurred in a subrandr function
    exit(1);

  sbr_subtitles *subs = sbr_load_file(sbr, "./my/subtitle/file.srv3");
  if(!subs)
    exit(1);
  
  sbr_renderer *renderer = sbr_renderer_create(sbr);
  if(!renderer)
    exit(1);

  sbr_subtitle_context ctx = {
    // this is **dots per inch**, not **pixels per inch**
    // if you have pixels per inch: dpi = ppi * 72 / 96
    .dpi = 144,
    .video_width = 1920.0,
    .video_height = 1080.0,
    .padding_left = 0.0,
    .padding_right = 0.0,
    .padding_top = 0.0,
    .padding_bottom = 0.0,
  };

  uint32_t t = 2424 /* milliseconds */;

  uint32_t width = 1920;
  uint32_t height = 1080;
  uint32_t *pixel_buffer = malloc(height * width * 4);
  if(!pixel_buffer) {
    printf("malloc failed\n");
    exit(1);
  }

  if(sbr_renderer_render(
    renderer,
    &ctx,
    t,
    subs,
    pixel_buffer,
    width,
    height
  ) < 0)
    exit(1);

  /* blit bitmap to the screen OVER the video */
  /* note: OVER is an alpha blending function */
  /* note: bitmap is already premultiplied, use premultiplied blending function */

  // some time later

  // you own the bitmap at all times and can free it whenever you want
  free(pixel_buffer);
  sbr_renderer_destroy(renderer);
  sbr_subtitles_destroy(subs);
  // same here, destroy the library last
  sbr_library_fini(sbr);
}
```
