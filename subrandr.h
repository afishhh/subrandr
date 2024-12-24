#ifndef SUBRANDR_H
#define SUBRANDR_H

#ifdef __cplusplus
#include <cstdint>

extern "C" {
#else
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

typedef struct sbr_subtitles sbr_subtitles;
typedef struct sbr_renderer sbr_renderer;
typedef struct sbr_subtitle_context {
  uint32_t dpi;
  float video_width, video_height;
  float padding_left, padding_right, padding_top, padding_bottom;
} sbr_subtitle_context;

typedef uint16_t SBR_UNSTABLE sbr_subtitle_format;
#define SBR_SUBTITLE_FORMAT_ASS (sbr_subtitle_format)1
#define SBR_SUBTITLE_FORMAT_SRV3 (sbr_subtitle_format)2

sbr_subtitles *sbr_load_file(char const *path);
void sbr_subtitles_destroy(sbr_subtitles *subs);

sbr_renderer *sbr_renderer_create(sbr_subtitles *subs);
int sbr_renderer_render(
    sbr_renderer *renderer, sbr_subtitle_context const *ctx,
    // current time value in milliseconds
    uint32_t t,
    // BGRA8 pixel buffer
    uint32_t *buffer, uint32_t width, uint32_t height
);
void sbr_renderer_destroy(sbr_renderer *renderer);

typedef uint32_t SBR_UNSTABLE sbr_error_code;
#define SBR_ERR_OTHER (sbr_error_code)1
#define SBR_ERR_IO (sbr_error_code)2
#define SBR_ERR_INVALID_ARGUMENT (sbr_error_code)3
#define SBR_ERR_UNRECOGNIZED_FILE (sbr_error_code)10

char const *sbr_get_last_error_string();
SBR_UNSTABLE uint32_t sbr_get_last_error_code();

#undef SBR_UNSTABLE

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_H
