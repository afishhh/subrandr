#ifndef SBR_FONT_PROVIDER_H
#define SBR_FONT_PROVIDER_H

#include "subrandr.h"

#ifdef __cplusplus
#include <cstddef>

extern "C" {
#else
#include <stddef.h>
#endif

typedef struct sbr_custom_font_provider sbr_custom_font_provider;

// Returns a new empty custom font provider.
sbr_custom_font_provider *sbr_custom_font_provider_create(void);

// Adds a single in-memory font to this font provider.
//
// Note that the font is immediately copied into a new buffer, parsed and the
// information within used to add to the internal index built by this font
// provider.
int sbr_custom_font_provider_add_from_memory(
    sbr_custom_font_provider *provider, const char *data, size_t data_len
);

// Adds all fonts found directly inside the specified directory.
//
// Note that the passed directory will be immediately scanned for fonts and all
// contained fonts opened and added to the internal index.
//
// Additionally, currently all fonts found within this directory will be kept
// open for the entire lifetime of this font provider. Clients passing untrusted
// paths to this function must consider the potential for Denial-of-Service
// caused by memory exhaustion if the directory contains very large fonts.
//
// The precise behavior of this function as described above is subject to
// change.
int sbr_custom_font_provider_add_all_from_dir(
    sbr_custom_font_provider *provider, const char *path
);

#ifdef __cplusplus
}
#endif

#endif // SBR_FONT_PROVIDER_H
