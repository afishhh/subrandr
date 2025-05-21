#ifdef SBR_ALLOW_UNSTABLE
#define SBR_UNSTABLE
#else
#define SBR_UNSTABLE                                                           \
  __attribute__((unavailable(                                                  \
      "This item is not part of subrandr's stable API yet.\n"                  \
      "Define SBR_ALLOW_UNSTABLE before including the header if "              \
      "you still want to use it."                                              \
  )))
#endif
