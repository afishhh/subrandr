#ifndef SUBRANDR_LAYOUT_H
#define SUBRANDR_LAYOUT_H
#include "subrandr.h"

#ifdef __cplusplus
extern "C" {
#endif

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct sbr_layout_context sbr_layout_context;

sbr_layout_context *sbr_layout_context_create(sbr_library *);
int sbr_layout_context_set_dpi(sbr_layout_context *, uint32_t dpi);
void sbr_layout_context_destroy(sbr_layout_context *);

typedef struct sbr_computed_style sbr_computed_style;

sbr_computed_style *sbr_computed_style_default(sbr_layout_context *);
sbr_computed_style *sbr_computed_style_compute_from_str(
    sbr_layout_context *, char const *declarations, size_t declarations_len,
    sbr_computed_style *parent);
void sbr_computed_style_ref(sbr_computed_style *);
void sbr_computed_style_unref(sbr_computed_style *);

typedef struct sbr_inline_builder sbr_inline_builder;
typedef struct sbr_span_builder sbr_span_builder;
typedef struct sbr_ruby_builder sbr_ruby_builder;
typedef struct sbr_block_builder sbr_block_builder;
typedef struct sbr_inline sbr_inline;
typedef struct sbr_block sbr_block;

sbr_block *sbr_block_from_inline(sbr_layout_context *, sbr_inline *,
                                 sbr_computed_style *);
sbr_block_builder *sbr_block_builder_create(sbr_layout_context *,
                                            sbr_computed_style *);
void sbr_block_builder_push(sbr_block_builder *, sbr_block *);
sbr_block *sbr_block_builder_finish(sbr_block_builder *);
void sbr_block_builder_set_style(sbr_inline_builder *, sbr_computed_style *);
void sbr_block_builder_destroy(sbr_block *, sbr_layout_context *);
void sbr_block_destroy(sbr_block *, sbr_layout_context *);

sbr_inline_builder *sbr_inline_builder_create(sbr_layout_context *,
                                              sbr_computed_style *);
sbr_span_builder *sbr_inline_builder_root(sbr_inline_builder *);
sbr_inline *sbr_inline_builder_finish(sbr_inline_builder *);
void sbr_inline_builder_set_style(sbr_inline_builder *, sbr_computed_style *);
void sbr_inline_builder_destroy(sbr_inline_builder *);
void sbr_inline_destroy(sbr_inline *, sbr_layout_context *);

int sbr_span_builder_append_text(sbr_span_builder *, char const *text,
                                 size_t text_len);
sbr_span_builder *sbr_span_builder_push_span(sbr_span_builder *,
                                             sbr_computed_style *);
void sbr_span_builder_finish(sbr_span_builder *);
sbr_ruby_builder *sbr_span_builder_push_ruby(sbr_span_builder *,
                                             sbr_computed_style *);

sbr_span_builder *sbr_ruby_builder_push_base(sbr_ruby_builder *,
                                             sbr_computed_style *);
sbr_span_builder *sbr_ruby_builder_push_annotation(sbr_ruby_builder *,
                                                   sbr_computed_style *);
void sbr_ruby_builder_finish(sbr_ruby_builder *);

typedef struct sbr_layout_pass sbr_layout_pass;
typedef struct sbr_fragment sbr_fragment;
typedef struct sbr_vec2l {
  sbr_26dot6 x, y;
} sbr_vec2l;

sbr_layout_pass *sbr_layout_pass_begin(sbr_layout_context *);
void sbr_layout_pass_end(sbr_layout_pass *);

sbr_fragment *sbr_block_layout(sbr_block *, sbr_layout_pass *,
                               sbr_vec2l available_size);
void sbr_fragment_destroy(sbr_fragment *);

sbr_vec2l sbr_fragment_size(sbr_fragment *);
sbr_instanced_raster_pass *sbr_fragment_render_instanced(sbr_fragment *,
                                                         sbr_layout_context *,
                                                         sbr_vec2l offset,
                                                         sbr_rect2i clip_rect,
                                                         uint64_t flags);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_LAYOUT_H
