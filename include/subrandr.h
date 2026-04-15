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

#define SUBRANDR_MAJOR SUBRANDR_MAJOR_PLACEHOLDER
#define SUBRANDR_MINOR SUBRANDR_MINOR_PLACEHOLDER
#define SUBRANDR_PATCH SUBRANDR_PATCH_PLACEHOLDER

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
typedef struct sbr_rect2i {
  int32_t min_x, min_y, max_x, max_y;
} sbr_rect2i;

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

// Iterator for subtitle events of an `sbr_subtitles` object.
//
// The size of this struct is not part of the public ABI,
// new fields may be added in ABI-compatible releases.
typedef struct sbr_subtitle_iterator {
  // Whether the iterator currently points to a subtitle event.
  //
  // Set to `true` once the end of an event list is reached and after
  // initialization.
  //
  // Accessors for subtitle event data like `sbr_subtitle_iterator_get_text`
  // must not be called if this flag is set.
  bool exhausted;

  // Start time of this subtitle event in milliseconds.
  uint32_t start;
  // End time of this subtitle event in milliseconds.
  uint32_t end;
} sbr_subtitle_iterator;

// Construct a new subtitle event iterator.
sbr_subtitle_iterator *sbr_subtitle_iterator_new(void);

// Advance this iterator to the next subtitle event.
//
// Note that the order of events in an `sbr_subtitles` object is unspecified
// so this event might not be after the current one time-wise.
void sbr_subtitle_iterator_next(sbr_subtitle_iterator *);

// Get the text content of this subtitle event.
//
// Text formatting will be ignored and not present in the result.
// Ruby will use parenthesized fallback form like "嗚呼(ああ)".
//
// `flags` must be zero.
//
// The returned string is guaranteed to be valid for as long as
// no other subrandr function is called on this iterator.
char const *sbr_subtitle_iterator_get_text(sbr_subtitle_iterator *,
                                           uint64_t flags);

// Stop iterating the subtitle event list the iterator currently points to.
//
// Should be done as soon as you are done using the iterator to avoid keeping
// resources alive unnecessarily as well as for future-proofing code to work
// with a future incremental parsing API.
void sbr_subtitle_iterator_reset(sbr_subtitle_iterator *);

void sbr_subtitle_iterator_destroy(sbr_subtitle_iterator *);

// Point the passed iterator to the beggining of the subtitle event list
// of this subtitle object.
//
// The order of events inside this internal list is unspecified.
void sbr_subtitles_iter(sbr_subtitles *, sbr_subtitle_iterator *);

#if 0 /* <-- so clangd doesn't think this is a doc comment :) */
// TODO: `sbr_subtitles_iter_at` would presumably be useful for some.
// void sbr_subtitles_iter_at(sbr_subtitle_iterator *, uint32_t point);
#endif

void sbr_subtitles_destroy(sbr_subtitles *);

sbr_renderer *sbr_renderer_create(sbr_library *);
void sbr_renderer_set_subtitles(sbr_renderer *, sbr_subtitles *);
bool sbr_renderer_did_change(sbr_renderer *, sbr_subtitle_context const *,
                             uint32_t t);

// Fully render a single subtitle frame into the provided pixel buffer.
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

// A single output image that resulted from instanced rendering of
// a subtitle frame.
//
// Note that even though this is called an "image" it may be a
// different output primitive internally that is not fully rasterized yet
// but whose bounding box is known and can be used for packing purposes.
//
// The size of this struct is not part of the public ABI,
// new fields may be added in ABI-compatible releases.
typedef struct sbr_output_image {
  uint32_t width, height;
  // This field is always NULL when returned by subrandr and isn't
  // read or modified by the library.
  // Can be used to associate custom data with images to simplify packing.
  void *user_data;
} sbr_output_image;

// A single instance that resulted from instanced rendering of
// a subtitle frame.
//
// Instances represent possibly scaled parts of output images.
// `base` refers to the image this instance is an instance of.
// `dst_x`, `dst_y` specify where in the output this instance
// should be composited to.
// `dst_width`, `dst_height` specify the size this instance's
// source image part needs to be scaled to.
// `src_off_x`, `src_off_y` specify at what offset in the
// base image this instance's source rectangle starts.
// `src_width`, `src_height` specify the size of this
// instance's source rectangle.
//
// To correctly composite an instance you must (conceptually):
// 1. Cut out the part of the source image covered by the source rectangle.
// 2. Scale the result to destination dimensions using bilinear interpolation.
// 3. Blend the result onto the output at the destination position.
//
// The size of this struct is not part of the public ABI,
// new fields may be added in ABI-compatible releases.
typedef struct sbr_output_instance {
  struct sbr_output_instance *next;
  struct sbr_output_image *base;
  int32_t dst_x, dst_y;
  uint32_t dst_width, dst_height;
  uint32_t src_off_x, src_off_y;
  uint32_t src_width, src_height;
} sbr_output_instance;

typedef struct sbr_instanced_raster_pass sbr_instanced_raster_pass;

// Render a single subtitle frame to output images, immediately
// begins an instanced raster pass which it returns a handle to.
//
// `clip_rect` is a rectangle that the resulting instances will be clipped to.
// `flags` must be zero.
//
// Calling `sbr_instanced_raster_pass_get_instances` on the resulting raster
// pass will yield a linked list of instances that must be composited onto the
// output to correctly display the subtitle frame.
// Instances must be composited in the order they are returned for correct
// results.
//
// See `sbr_renderer_render` for details on other parameters.
sbr_instanced_raster_pass *
sbr_renderer_render_instanced(sbr_renderer *, sbr_subtitle_context const *,
                              uint32_t t, sbr_rect2i clip_rect, uint64_t flags);

// Returns the first element of the internal list of output image instances
// that are to be drawn during this raster pass.
sbr_output_instance *
sbr_instanced_raster_pass_get_instances(sbr_instanced_raster_pass *);

// Rasterize this output image into the provided pixel buffer at
// a specified offset.
//
// `off_x` and `off_y` specify the offset at which to draw the image.
// These values may be negative or otherwise out of bounds of the
// output buffer and the result will be appropriately clipped.
//
// The pixel buffer is provided in `buffer`, `width`, `height`, and `stride`
// like in `sbr_renderer_render`.
int sbr_output_image_rasterize_into(sbr_output_image const *,
                                    sbr_instanced_raster_pass *, int32_t off_x,
                                    int32_t off_y, sbr_bgra8 *buffer,
                                    uint32_t width, uint32_t height,
                                    uint32_t stride);

// Mark the provided raster pass as finished.
//
// Must be called before the parent renderer can be used again.
// Failing to do so will currently result in an assertion failure.
void sbr_instanced_raster_pass_finish(sbr_instanced_raster_pass *);

void sbr_renderer_destroy(sbr_renderer *);

// Get the error message of the last error that occurred on the current thread.
// The returned string will be valid until another error occurs on this thread.
//
// Returns `NULL` if no error has occurred on this thread yet.
char const *sbr_get_last_error_string(void);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_H
