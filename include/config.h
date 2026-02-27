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

#define SBR_ERR_OPTION_NOT_FOUND (-(int)10)

int sbr_config_set_str(sbr_config *, const char *name, const char *value);

void sbr_config_destroy(sbr_config *);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_CONFIG_H
