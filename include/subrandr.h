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

// Renders a single subtitle frame to output images and immediately
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

// Rasterizes this output image into the provided pixel buffer at
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

// Marks the provided raster pass as finished.
//
// Must be called before the parent renderer can be used again.
// Failing to do so will currently result in an assertion failure.
void sbr_instanced_raster_pass_finish(sbr_instanced_raster_pass *);

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
