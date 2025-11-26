#ifndef SUBRANDR_PIECE_RENDER_H
#define SUBRANDR_PIECE_RENDER_H

#include "subrandr.h"

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>

extern "C" {
#else
#include <stddef.h>
#include <stdint.h>
#endif

typedef enum {
  SBR_OUTPUT_PIECE_IMAGE_A8 = 0,
  SBR_OUTPUT_PIECE_IMAGE_BGRA8 = 1,
} sbr_output_piece_kind;

typedef uint32_t sbr_rgba32;

typedef struct {
  int32_t x, y;
  uint32_t width, height, stride;
  union {
    struct {
      uint8_t *buffer;
      sbr_rgba32 color;
    } a8;

    struct {
      uint32_t *buffer;
      uint8_t alpha;
    } bgra8;
  };
} sbr_output_piece_image;

typedef struct _sbr_output_piece {
  sbr_output_piece_kind kind;
  struct _sbr_output_piece *next;
  union {
    sbr_output_piece_image image;
  };
} sbr_output_piece;

typedef uint32_t sbr_piece_render_flags;

sbr_output_piece const *
sbr_renderer_render_pieces(sbr_renderer *, sbr_subtitle_context const *,
                           uint32_t t, sbr_piece_render_flags flags);

#ifdef __cplusplus
}
#endif

#endif
