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
#else
#define SBR_UNSTABLE                                                           \
  __attribute__((                                                              \
      unavailable("This item is not part of subrandr's stable API yet.\n"      \
                  "Define SBR_ALLOW_UNSTABLE before including subrandr.h if "  \
                  "you still want to use it.")                                 \
  ))
#endif

typedef struct sbr_library sbr_library;
typedef struct sbr_subtitles sbr_subtitles;
typedef struct sbr_renderer sbr_renderer;
typedef int32_t sbr_26dot6;
typedef struct sbr_subtitle_context {
  uint32_t dpi;
  sbr_26dot6 video_width, video_height;
  sbr_26dot6 padding_left, padding_right, padding_top, padding_bottom;
} sbr_subtitle_context;

sbr_library *sbr_library_init(void);
void sbr_library_fini(sbr_library *);

typedef int16_t sbr_subtitle_format;
#define SBR_SUBTITLE_FORMAT_UNKOWN (sbr_subtitle_format)0
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
sbr_subtitles *sbr_load_text(
    sbr_library *, char const *content, size_t content_len,
    sbr_subtitle_format format, char const *language_hint
);

// TODO: Remove this. It shouldn't really exist or at least should
//       probably have different semantics.
SBR_UNSTABLE sbr_subtitles *sbr_load_file(sbr_library *, char const *path);

void sbr_subtitles_destroy(sbr_subtitles *);

sbr_renderer *sbr_renderer_create(sbr_library *);
bool sbr_renderer_did_change(
    sbr_renderer *, sbr_subtitle_context const *, uint32_t t
);
int sbr_renderer_render(
    sbr_renderer *, sbr_subtitle_context const *,
    // subtitles to render, on change invalidate the cache before rendering
    sbr_subtitles *,
    // current time value in milliseconds
    uint32_t t,
    // BGRA8 pixel buffer
    uint32_t *buffer, uint32_t width, uint32_t height
);
void sbr_renderer_destroy(sbr_renderer *);

typedef uint32_t SBR_UNSTABLE sbr_error_code;
#define SBR_ERR_OTHER (sbr_error_code)1
#define SBR_ERR_IO (sbr_error_code)2
#define SBR_ERR_INVALID_ARGUMENT (sbr_error_code)3
#define SBR_ERR_UNRECOGNIZED_FORMAT (sbr_error_code)10

char const *sbr_get_last_error_string(void);
SBR_UNSTABLE uint32_t sbr_get_last_error_code(void);

#undef SBR_UNSTABLE

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_H
