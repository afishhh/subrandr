#ifndef SUBRANDR_CONFIG_H
#define SUBRANDR_CONFIG_H

#include "subrandr.h"

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>

extern "C" {
#else
#include <stddef.h>
#include <stdint.h>
#endif

sbr_config *sbr_config_new(sbr_library *);

// naming is hard and shed color not decided upon
typedef uint32_t sbr_bikeshed_result;
#define SBR_BIKESHED_NOT_FOUND ((sbr_bikeshed_result)1)
#define SBR_BIKESHED_INVALID_VALUE ((sbr_bikeshed_result)2)

// TODO: bikeshed
sbr_bikeshed_result sbr_config_set_str(sbr_config *, const char *name,
                                       const char *value, size_t value_len);

void sbr_config_destroy(sbr_config *);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_CONFIG_H
