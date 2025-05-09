#ifndef SUBRANDR_VK_H
#define SUBRANDR_VK_H

#include "subrandr.h"

#ifdef __cplusplus
#include <cstddef>
#include <cstdint>

extern "C" {
#else
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#endif

#include <vulkan/vulkan_core.h>

#include "internal/stability.h"

typedef SBR_UNSTABLE uint64_t sbr_vk_flags;
typedef SBR_UNSTABLE struct sbr_vk_entry sbr_vk_entry;
typedef SBR_UNSTABLE struct sbr_vk_instance sbr_vk_instance;
typedef SBR_UNSTABLE struct sbr_vk_adapter sbr_vk_adapter;
typedef SBR_UNSTABLE struct sbr_vk_physical_device_features
    sbr_vk_physical_device_features;
typedef SBR_UNSTABLE struct sbr_vk_device sbr_vk_device;
typedef SBR_UNSTABLE struct sbr_vk_rasterizer sbr_vk_rasterizer;
typedef SBR_UNSTABLE struct sbr_vk_render_target sbr_vk_render_target;

typedef SBR_UNSTABLE void (*sbr_vk_proc_addr)(void);
typedef SBR_UNSTABLE
    sbr_vk_proc_addr (*sbr_vk_get_instance_proc_addr)(void *, char const *);

SBR_UNSTABLE sbr_vk_entry *sbr_vk_entry_create(
    sbr_library *, sbr_vk_get_instance_proc_addr get_instance_proc_addr
);

SBR_UNSTABLE int sbr_vk_entry_desired_extensions(
    sbr_vk_entry *, sbr_vk_flags, char const *const **extensions,
    size_t *num_extensions
);

SBR_UNSTABLE void sbr_vk_entry_destroy(sbr_vk_entry *);

// More fields can be appended to this struct in the future that get
// conditionally read depending on `flags`, don't assume it won't change!
typedef SBR_UNSTABLE struct sbr_vk_instance_params {
  SBR_UNSTABLE sbr_vk_flags flags;
  char const *const *extensions;
  size_t num_extensions;
  uint32_t android_sdk_version;
} sbr_vk_instance_params;

SBR_UNSTABLE sbr_vk_instance *sbr_vk_instance_create(
    sbr_vk_entry *, VkInstance, sbr_vk_instance_params const *
);

SBR_UNSTABLE void sbr_vk_instance_destroy(sbr_vk_instance *);

SBR_UNSTABLE sbr_vk_adapter *
sbr_vk_adapter_create(sbr_vk_instance *, VkPhysicalDevice);

SBR_UNSTABLE sbr_vk_physical_device_features *
sbr_vk_adapter_required_physical_device_features(
    sbr_vk_adapter *, sbr_vk_flags
);

SBR_UNSTABLE void sbr_vk_adapter_destroy(sbr_vk_adapter *);

SBR_UNSTABLE void sbr_vk_physical_device_features_required_extensions(
    sbr_vk_physical_device_features *, char const *const **extensions,
    size_t *num_extensions
);

SBR_UNSTABLE void sbr_vk_physical_device_features_add_to_device_create(
    sbr_vk_physical_device_features *, VkDeviceCreateInfo *
);

SBR_UNSTABLE void
sbr_vk_physical_device_features_destroy(sbr_vk_physical_device_features *);

// More fields can be appended to this struct in the future that get
// conditionally read depending on `flags`, don't assume it won't change!
typedef SBR_UNSTABLE struct sbr_vk_device_params {
  SBR_UNSTABLE sbr_vk_flags flags;
  char const *const *enabled_extensions;
  size_t num_enabled_extensions;
  uint32_t family_index;
  uint32_t queue_index;
} sbr_vk_device_params;

SBR_UNSTABLE sbr_vk_device *sbr_vk_device_from_raw(
    sbr_vk_adapter *, VkDevice, sbr_vk_device_params const *
);

SBR_UNSTABLE void sbr_vk_device_destroy(sbr_vk_device *);

SBR_UNSTABLE sbr_vk_rasterizer *sbr_vk_rasterizer_create(sbr_vk_device *);

SBR_UNSTABLE sbr_vk_render_target *sbr_vk_rasterizer_create_render_target(
    sbr_vk_rasterizer *, VkImage, VkExtent2D const *
);

SBR_UNSTABLE int
sbr_vk_rasterizer_submit(sbr_vk_rasterizer *, sbr_vk_render_target *);

SBR_UNSTABLE int sbr_vk_rasterizer_destroy_render_target(
    sbr_vk_rasterizer *, sbr_vk_render_target *
);

SBR_UNSTABLE void sbr_vk_rasterizer_destroy(sbr_vk_rasterizer *);

#undef SBR_UNSTABLE

#ifdef __cplusplus
}
#endif

#endif // SUBRANDR_VK_H
