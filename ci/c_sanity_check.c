#include <stdint.h>
#include <stdio.h>

#include "subrandr/logging.h"
#include "subrandr/subrandr.h"

int main() {
  uint32_t a, b, c;
  sbr_library_version(&a, &b, &c);
  printf("subrandr runtime version: v%u.%u.%u\n", a, b, c);
  printf("subrandr header version: v%u.%u.%u\n", SUBRANDR_MAJOR, SUBRANDR_MINOR,
         SUBRANDR_PATCH);

  sbr_library *lib = sbr_library_init();
  sbr_renderer *r = sbr_renderer_create(lib);
  printf("renderer %p created\n", r);
  sbr_renderer_destroy(r);
  sbr_library_fini(lib);

  // so logging.h is used and we can ensure its presence
  (void)SBR_LOG_LEVEL_DEBUG;
}
