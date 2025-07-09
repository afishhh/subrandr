#include <stdint.h>
#include <stdio.h>

#include "subrandr/subrandr.h"

int main() {
  uint32_t a, b, c;
  sbr_library_version(&a, &b, &c);
  printf("subrandr v%u.%u.%u\n", a, b, c);

  sbr_library *lib = sbr_library_init();
  sbr_renderer *r = sbr_renderer_create(lib);
  printf("renderer %p created\n", r);
  sbr_renderer_destroy(r);
  sbr_library_fini(lib);
}
