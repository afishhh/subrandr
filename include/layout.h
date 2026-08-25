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
// TODO: consider function for getting just derived style (with no declarations)
//       you can always just do from_str(lctx, "", 0, parent) so it's not
//       anything new
sbr_computed_style *sbr_computed_style_compute_from_str(
    sbr_layout_context *, char const *declarations, size_t declarations_len,
    sbr_computed_style *parent);
void sbr_computed_style_ref(sbr_computed_style *);
void sbr_computed_style_unref(sbr_computed_style *);

typedef struct sbr_image sbr_image;

sbr_image *sbr_image_from_bgra(sbr_library *, void *data, uint32_t width,
                               uint32_t height, uint32_t stride);
// TODO: image from outline
//       or image from callback (callback would receive an `sbr_scene_builder`
//       and would be able to insert scene nodes like outlines or rectanges n
//       stuff)
void sbr_image_ref(sbr_image *);
void sbr_image_unref(sbr_image *);

typedef struct sbr_vec2l {
  sbr_26dot6 x, y;
} sbr_vec2l;

typedef struct sbr_inline_builder sbr_inline_builder;
typedef struct sbr_span_builder sbr_span_builder;
typedef struct sbr_ruby_builder sbr_ruby_builder;
typedef struct sbr_block_builder sbr_block_builder;
// TODO: better name
typedef struct sbr_custom_container_builder sbr_custom_container_builder;

// A single box in the box tree.
//
// Boxes are entities generated for elements based on the `display` property
// before laying out a document. Boxes may have children which are laid out
// according to the box's inner layout algorithm.
//
// Once constructed, a box cannot be mutated. In order to modify a box
// you need to recreate the appropriate branch of the layout tree in its
// entirety.
//
// Note that the functions provided by this API abstract away some box
// construction and box types. For example, it is not possible to observe
// any internal ruby box as an `sbr_box`.
// This type is only used for "standalone" boxes (ones not tied to any
// particular outer layout algorithm).
//
// See https://drafts.csswg.org/css-display/#box-tree for more details
// on the CSS box tree.
typedef struct sbr_box sbr_box;

// TODO: reconsider lctx argument. it isn't used by current impl but might be
//       useful in another possible one
void sbr_box_destroy(sbr_box *, sbr_layout_context *);

// Create a box for a replaced element with the specified contents.
sbr_box *sbr_box_from_image(sbr_layout_context *, sbr_image *,
                            sbr_computed_style *);

sbr_block_builder *sbr_block_builder_create(sbr_layout_context *,
                                            sbr_computed_style *);
void sbr_block_builder_push(sbr_block_builder *, sbr_box *);
// TODO: should container builders be automatically destroyed on `_finish()`?
sbr_box *sbr_block_builder_finish(sbr_block_builder *);
void sbr_block_builder_set_style(sbr_block_builder *, sbr_computed_style *);
void sbr_block_builder_destroy(sbr_block_builder *);

// Create a new inline builder that can be used to build up inline
// content and finalize it into a block box.
//
// You can add content to this builder by first acquiring the root builder
// via `sbr_inline_builder_root`.
// Construction can be finalized via `sbr_inline_builder_finish_block`
// which will return a block wrapper box containing the inline content.
//
// Inline content cannot be constructed incrementally. It is not
// and will not be possible to join or nest two inline builders.
sbr_inline_builder *sbr_inline_builder_create(sbr_layout_context *,
                                              sbr_computed_style *);

// Get the root inline span builder of this box.
//
// This builder inserts inline content directly into the anonymous
// root inline box.
//
// See https://drafts.csswg.org/css-inline/#root-inline-box.
sbr_span_builder *sbr_inline_builder_root(sbr_inline_builder *);

// Create a block box with the inline content from this builder.
//
// Since a block cannot contain mixed block-level and inline-level
// children, an inline box is not "standalone" and cannot be its own
// `sbr_box`; hence this function returns a block-level wrapper
// instead of the inline directly.
//
// Resets the builder allowing it to be reused.
sbr_box *sbr_inline_builder_finish_block(sbr_inline_builder *);

// Change the style of an empty inline builder.
//
// Can only be called on an empty builder and may cause an assertion failure if
// this invariant is violated.
void sbr_inline_builder_set_style(sbr_inline_builder *, sbr_computed_style *);

// Destroy an inline builder.
void sbr_inline_builder_destroy(sbr_inline_builder *);

// TODO: make below append/push functions have consistent doc style

// Append text content into the span.
//
// Fails if the passed buffer is not valid UTF-8.
int sbr_span_builder_append_text(sbr_span_builder *, char const *text,
                                 size_t text_len);

// Append a box as an atomic inline.
//
// The box will be considered inline-level and sized accordingly.
void sbr_span_builder_push_atomic(sbr_span_builder *, sbr_box *);

// Append a new span into the span being constructed by this builder.
//
// This will result in a new span builder being created which allows
// you to insert content into the child span.
// Pushing additional content into the parent span before finalizing
// the child builder is not allowed.
// TODO: Guarantee assertion or allow undefined behavior?
sbr_span_builder *sbr_span_builder_push_span(sbr_span_builder *,
                                             sbr_computed_style *);

// Append a new ruby container into the span.
//
// This will create a new ruby builder through which you can insert content
// into the ruby container.
// Pushing additional content into the parent span before finalizing
// the child builder is not allowed.
sbr_ruby_builder *sbr_span_builder_push_ruby(sbr_span_builder *,
                                             sbr_computed_style *);

void sbr_span_builder_finish(sbr_span_builder *);

// Push a new ruby base into the ruby container.
//
// This will result in a new span builder being created which allows
// you to insert content into the ruby base box.
// Pushing additional content into the parent ruby container before finalizing
// the child builder is not allowed.
sbr_span_builder *sbr_ruby_builder_push_base(sbr_ruby_builder *,
                                             sbr_computed_style *);

// Push a new ruby annotation into the ruby container.
//
// This will result in a new span builder being created which allows
// you to insert content into the ruby annotation box.
// Pushing additional content into the parent ruby container before finalizing
// the child builder is not allowed.
sbr_span_builder *sbr_ruby_builder_push_annotation(sbr_ruby_builder *,
                                                   sbr_computed_style *);

void sbr_ruby_builder_finish(sbr_ruby_builder *);

typedef struct sbr_layout_pass sbr_layout_pass;
// TODO: box_fragment or fragment? make functions consistent with choice
typedef struct sbr_box_fragment sbr_box_fragment;

sbr_layout_pass *sbr_layout_pass_begin(sbr_layout_context *);
void sbr_layout_pass_end(sbr_layout_pass *);

sbr_custom_container_builder *
sbr_custom_container_builder_create(sbr_layout_pass *, sbr_computed_style *);
// TODO: really this should take a fragment instead
int sbr_custom_container_builder_place(sbr_custom_container_builder *,
                                       sbr_vec2l offset, sbr_box *,
                                       sbr_vec2l size);
sbr_box *sbr_custom_container_builder_finish(sbr_custom_container_builder *,
                                             sbr_vec2l container_size);
void sbr_custom_container_builder_set_style(sbr_inline_builder *,
                                            sbr_computed_style *);
void sbr_custom_container_builder_destroy(sbr_custom_container_builder *);

// Return size measurement for the X-axis.
//
// If not set, the returned width is unspecified.
#define SBR_MEASURE_WIDTH (1 << 0)

// Return size measurement for the Y-axis.
//
// If not set, the returned height is unspecified.
#define SBR_MEASURE_HEIGHT (1 << 1)

// TODO: make this write to an sbr_measure_result where more fields
//       can be added (like baselines)?
//       but baselines would require a fragment internally... hmmm
int sbr_box_measure(sbr_box *, sbr_layout_pass *, sbr_vec2l *out,
                    sbr_vec2l constraints, uint64_t flags);

// TODO: should this even have an "available size?"
sbr_box_fragment *sbr_box_layout(sbr_box *, sbr_layout_pass *,
                                 sbr_vec2l available_size);

sbr_vec2l sbr_fragment_size(sbr_box_fragment *);
void sbr_fragment_destroy(sbr_box_fragment *);

typedef struct sbr_scene sbr_scene;

void sbr_scene_destroy(sbr_scene *);

typedef struct sbr_display_pass sbr_display_pass;

sbr_display_pass *sbr_display_pass_begin(sbr_layout_context *);

int sbr_fragment_display(sbr_box_fragment *, sbr_display_pass *,
                         sbr_vec2l offset);

#if 0
// TODO: allow interupting display pass?
#endif

sbr_scene *sbr_display_pass_finish(sbr_display_pass *);

typedef struct sbr_sw_rasterizer sbr_sw_rasterizer;

sbr_sw_rasterizer *sbr_sw_rasterizer_create(sbr_library *);
int sbr_sw_rasterizer_bad_render_dont_commit(sbr_sw_rasterizer *, sbr_scene *,
                                             sbr_bgra8 *buffer, uint32_t width,
                                             uint32_t height, uint32_t stride);
sbr_instanced_raster_pass *
sbr_sw_rasterizer_render_instanced(sbr_sw_rasterizer *, sbr_scene *,
                                   sbr_rect2i clip_rect, uint64_t flags);
void sbr_sw_rasterizer_destroy(sbr_sw_rasterizer *);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_LAYOUT_H
