#ifndef SUBRANDR_H
#define SUBRANDR_H

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>

extern "C" {
#else
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#endif

#ifdef SBR_ALLOW_UNSTABLE
#define SBR_UNSTABLE
// clang implements this attribute sensibly and doesn't cause unavailable errors
// if an unavailable type is only used in other unavailable declarations.
// GCC does not have these semantics and I'm scared of what other compilers
// might do here.
#elif defined(__has_attribute) && defined(__clang__)
#if __has_attribute(unavailable)
#define SBR_UNSTABLE                                                           \
  __attribute__((                                                              \
      unavailable("This item is not part of subrandr's stable API yet. "       \
                  "Define SBR_ALLOW_UNSTABLE before including subrandr.h if "  \
                  "you still want to use it.")))
#endif
#endif

typedef int32_t sbr_26dot6;
typedef uint32_t sbr_bgra8;

typedef struct sbr_library sbr_library;
typedef struct sbr_subtitles sbr_subtitles;
typedef struct sbr_renderer sbr_renderer;
typedef struct sbr_subtitle_context {
  uint32_t dpi;
  sbr_26dot6 video_width, video_height;
  sbr_26dot6 padding_left, padding_right, padding_top, padding_bottom;
} sbr_subtitle_context;

// Construct a new `sbr_library *` that is required to use most subrandr APIs.
//
// `sbr_library` also allows you to set a log callback via
// `sbr_library_set_log_callback`.
sbr_library *sbr_library_init(void);

// Finalize an `sbr_library *` object.
//
// Must only be called after all other subrandr objects created from this
// library object are destroyed.
void sbr_library_fini(sbr_library *);

void sbr_library_version(uint32_t *major, uint32_t *minor, uint32_t *patch);

typedef int16_t sbr_subtitle_format;
#define SBR_SUBTITLE_FORMAT_UNKNOWN (sbr_subtitle_format)0
#define SBR_SUBTITLE_FORMAT_SRV3 (sbr_subtitle_format)1
#define SBR_SUBTITLE_FORMAT_WEBVTT (sbr_subtitle_format)2

// Probe subtitle text for a matching format magic.
//
// This function tries to determine the subtitle format of `content`
// on a best-effort basis.
sbr_subtitle_format sbr_probe_text(char const *content, size_t content_len);

// Load subtitles from text data.
//
// If `format` is not SBR_SUBTITLE_FORMAT_UNKNOWN then subrandr will assume
// subtitles are in the given subtitle format and parse them accordingly.
// Otherwise the format will first be probed in the same manner as
// `sbr_probe_text`.
//
// If `language_hint` is not NULL then the given subtitles will be assumed to
// be in the given language. For example, it may be treated as the
// "default language" in WebVTT.
sbr_subtitles *sbr_load_text(sbr_library *, char const *content,
                             size_t content_len, sbr_subtitle_format format,
                             char const *language_hint);

void sbr_subtitles_destroy(sbr_subtitles *);

sbr_renderer *sbr_renderer_create(sbr_library *);
void sbr_renderer_set_subtitles(sbr_renderer *, sbr_subtitles *);
bool sbr_renderer_did_change(sbr_renderer *, sbr_subtitle_context const *,
                             uint32_t t);

// Fully renders a single subtitle frame into the provided pixel buffer.
//
// Renders the subtitles that were previously specified with
// `sbr_renderer_set_subtitles` at the state they should be at millisecond `t`.
//
// The pixel buffer to render to is specified via `buffer`, `width`,
// `height`, and `stride`. `width` and `height` should be set to the
// pixel dimensions of the buffer pointed to by `buffer`. `stride` should be set
// to the pixel stride of the `buffer`, note that this value is in 32-bit pixel
// units and not bytes.
//
// The passed in `sbr_subtitle_context const *` determines what *logical*
// parameters should be used for *layout*. This is strictly different from the
// dimensions of the pixel buffer itself which only affect rasterization.
int sbr_renderer_render(sbr_renderer *, sbr_subtitle_context const *,
                        uint32_t t, sbr_bgra8 *buffer, uint32_t width,
                        uint32_t height, uint32_t stride);

#ifdef SBR_UNSTABLE
// Structure representing a single output piece that resulted from
// fragmented rendering of a subtitle frame.
//
// Pieces are output primitives that may not be fully rasterized yet,
// but whose bounding box is known and can be used for packing purposes.
//
// The size of this struct is not part of the public ABI,
// new fields may be added in ABI-compatible releases.
typedef SBR_UNSTABLE struct sbr_output_piece {
  int32_t x, y;
  uint32_t width, height;
  struct sbr_output_piece *next;
} sbr_output_piece;

typedef SBR_UNSTABLE struct sbr_piece_raster_pass sbr_piece_raster_pass;

// Renders a single subtitle frame to output pieces and immediately
// begins a piece raster pass which it returns a handle to.
//
// See `sbr_renderer_render` for details on parameters.
sbr_piece_raster_pass *sbr_renderer_render_pieces(sbr_renderer *,
                                                  sbr_subtitle_context const *,
                                                  uint32_t t) SBR_UNSTABLE;

// Returns the first element of the internal list of output pieces
// that are to be drawn during this raster pass.
sbr_output_piece const *
sbr_piece_raster_pass_get_pieces(sbr_piece_raster_pass *) SBR_UNSTABLE;

// Rasterizes the provided output piece into the provided pixel buffer at
// a specified offset.
//
// `off_x` and `off_y` specify the offset at which to draw the piece.
// These values may be negative or otherwise out of bounds of the
// output buffer and the result will be appropriately clipped.
//
// The pixel buffer is provided in `buffer`, `width`, `height`, and `stride`
// same as in `sbr_renderer_render`.
int sbr_piece_raster_pass_draw_piece(sbr_piece_raster_pass *,
                                     sbr_output_piece const *piece,
                                     int32_t off_x, int32_t off_y,
                                     sbr_bgra8 *buffer, uint32_t width,
                                     uint32_t height,
                                     uint32_t stride) SBR_UNSTABLE;

// Marks the provided raster pass as finished.
//
// Note that calling this function after a raster pass is *mandatory*,
// currently failing to do so will be met with an assertion failure.
void sbr_piece_raster_pass_finish(sbr_piece_raster_pass *) SBR_UNSTABLE;
#endif

void sbr_renderer_destroy(sbr_renderer *);

#ifdef SBR_UNSTABLE
typedef uint32_t SBR_UNSTABLE sbr_error_code;
#define SBR_ERR_OTHER (sbr_error_code)1
#define SBR_ERR_IO (sbr_error_code)2
#define SBR_ERR_INVALID_ARGUMENT (sbr_error_code)3
#define SBR_ERR_UNRECOGNIZED_FORMAT (sbr_error_code)10
#endif

char const *sbr_get_last_error_string(void);
#ifdef SBR_UNSTABLE
SBR_UNSTABLE uint32_t sbr_get_last_error_code(void);
#endif

#undef SBR_UNSTABLE

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_H
