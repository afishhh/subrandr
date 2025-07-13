#ifndef SUBRANDR_LOGGING_H
#define SUBRANDR_LOGGING_H

#include "subrandr.h"

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>

extern "C" {
#else
#include <stddef.h>
#include <stdint.h>
#endif

#define SBR_LOG_LEVEL_TRACE (uint8_t)0
#define SBR_LOG_LEVEL_DEBUG (uint8_t)1
#define SBR_LOG_LEVEL_INFO (uint8_t)2
#define SBR_LOG_LEVEL_WARN (uint8_t)3
#define SBR_LOG_LEVEL_ERROR (uint8_t)4

// One of `SBR_LOG_LEVEL_{TRACE,DEBUG,INFO,WARN,ERROR}`.
//
// New variants may be added in the future.
// Users should treat values >ERROR as ERROR for forwards compatibility.
typedef uint8_t sbr_log_level;

// Callback for library log messages.
//
// `level` is the severity of the message.
// `source` is a string describing the origin of the error within the library.
// `message` is the message itself.
//
// These strings are not null-terminated, the corresponding `_len` argument must
// be used to avoid overruns. Do not rely on the contents of these strings.
typedef void (*sbr_log_callback)(
    sbr_log_level, char const *source, size_t source_len, char const *message,
    size_t message_len, void *user_data
);

// Set a callback for subrandr log messages.
//
// Can only be called before any renderers are created.
// Note that calling this after a renderer has been created is currently
// *UNSOUND* even if done in a thread-safe manner.
// This restriction may be relaxed in the future.
void sbr_library_set_log_callback(
    sbr_library *, sbr_log_callback, void *user_data
);

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_LOGGING_H
