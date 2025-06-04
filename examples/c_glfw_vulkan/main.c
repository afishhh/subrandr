#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include <vulkan/vulkan_core.h>

#define GLFW_INCLUDE_VULKAN
#include <GLFW/glfw3.h>

#define SBR_ALLOW_UNSTABLE
#include <subrandr/subrandr.h>
#include <subrandr/vk.h>

#define ASSERT_VK(expr)                                                        \
  {                                                                            \
    VkResult result = expr;                                                    \
    if (result != VK_SUCCESS) {                                                \
      eprintf(#expr "\n");                                                     \
      panicf("returned vk error %i\n", result);                                \
    }                                                                          \
  }

#define eprintf(...) fprintf(stderr, ##__VA_ARGS__)
#define panicf(fmt, ...)                                                       \
  {                                                                            \
    eprintf(fmt "\n", ##__VA_ARGS__);                                          \
    exit(1);                                                                   \
  }

struct vk_global_procs {
  PFN_vkCreateInstance createInstance;
  PFN_vkEnumerateInstanceExtensionProperties
      enumerateInstanceExtensionProperties;
};

GLFWvkproc checked_get_vk_proc_addr(VkInstance instance, char const *procname) {
  void *ptr = glfwGetInstanceProcAddress(instance, procname);
  if (!ptr) {
    panicf(
        "Failed to get address for vulkan procedure \"%s\" for instance %p\n",
        procname, instance
    );
  }
  return ptr;
}

struct vk_global_procs get_vk_global_procs() {
  return (struct vk_global_procs){
      // clang-format off
      .createInstance = (PFN_vkCreateInstance)checked_get_vk_proc_addr(NULL, "vkCreateInstance"),
      .enumerateInstanceExtensionProperties = (PFN_vkEnumerateInstanceExtensionProperties)checked_get_vk_proc_addr(NULL, "vkEnumerateInstanceExtensionProperties"),
      // clang-format on
  };
}

struct vk_instance_procs {
  PFN_vkEnumeratePhysicalDevices enumeratePhysicalDevices;
  PFN_vkGetPhysicalDeviceQueueFamilyProperties
      getPhysicalDeviceQueueFamilyProperties;
  PFN_vkCreateDevice createDevice;
  PFN_vkCreateSwapchainKHR createSwapchainKHR;
  PFN_vkDestroySwapchainKHR destroySwapchainKHR;
  PFN_vkAcquireNextImageKHR acquireNextImageKHR;
  PFN_vkCreateSemaphore createSemaphore;
  PFN_vkGetSwapchainImagesKHR getSwapchainImagesKHR;
  PFN_vkQueuePresentKHR queuePresentKHR;
  PFN_vkGetDeviceQueue getDeviceQueue;
};

struct vk_instance_procs get_vk_instance_procs(VkInstance instance) {
  // clang-format off
  return (struct vk_instance_procs){
    .enumeratePhysicalDevices = (PFN_vkEnumeratePhysicalDevices)checked_get_vk_proc_addr(instance, "vkEnumeratePhysicalDevices"),
    .getPhysicalDeviceQueueFamilyProperties = (PFN_vkGetPhysicalDeviceQueueFamilyProperties)checked_get_vk_proc_addr(instance, "vkGetPhysicalDeviceQueueFamilyProperties"),
    .createDevice = (PFN_vkCreateDevice)checked_get_vk_proc_addr(instance, "vkCreateDevice"),
    .createSwapchainKHR = (PFN_vkCreateSwapchainKHR)checked_get_vk_proc_addr(instance, "vkCreateSwapchainKHR"),
    .destroySwapchainKHR = (PFN_vkDestroySwapchainKHR)checked_get_vk_proc_addr(instance, "vkDestroySwapchainKHR"),
    .acquireNextImageKHR = (PFN_vkAcquireNextImageKHR)checked_get_vk_proc_addr(instance, "vkAcquireNextImageKHR"),
    .createSemaphore = (PFN_vkCreateSemaphore)checked_get_vk_proc_addr(instance, "vkCreateSemaphore"),
    .getSwapchainImagesKHR = (PFN_vkGetSwapchainImagesKHR )checked_get_vk_proc_addr(instance, "vkGetSwapchainImagesKHR"),
    .queuePresentKHR = (PFN_vkQueuePresentKHR)checked_get_vk_proc_addr(instance, "vkQueuePresentKHR"),
    .getDeviceQueue = (PFN_vkGetDeviceQueue)checked_get_vk_proc_addr(instance, "vkGetDeviceQueue"),
  };
  // clang-format on
}

typedef struct stringvec {
  char const **data;
  size_t capacity;
  size_t len;
} stringvec;

stringvec svec_alloc(size_t capacity) {
  return (stringvec){.data = malloc(sizeof(char const *) * capacity),
                     .capacity = capacity,
                     .len = 0};
}

void svec_set_insert(stringvec *svec, char const *item) {
  for (size_t i = 0; i < svec->len; ++i)
    if (!strcmp(svec->data[i], item))
      return;
  svec->data[svec->len++] = item;
}

void svec_free(stringvec *svec) {
  free(svec->data);
  svec->capacity = 0;
  svec->len = 0;
}

typedef struct vk_state {
  struct vk_global_procs global_procs;
  struct vk_instance_procs instance_procs;
  VkInstance instance;
  VkPhysicalDevice physical_device;
  VkExtensionProperties *supported_extensions;
  uint32_t supported_extension_count;
  uint32_t queue_family;
  VkDevice device;
  stringvec device_extensions;
  VkQueue queue;
} vk_state;

void vk_init(struct vk_state *vk) {
  vk->global_procs = get_vk_global_procs();
  vk->instance = NULL;
  vk->physical_device = VK_NULL_HANDLE;
  vk->device = VK_NULL_HANDLE;
  vk->queue = VK_NULL_HANDLE;
  vk->supported_extensions = NULL;
  vk->device_extensions.data = NULL;

  ASSERT_VK(vk->global_procs.enumerateInstanceExtensionProperties(
      NULL, &vk->supported_extension_count, NULL
  ));

  vk->supported_extensions =
      malloc(sizeof(VkExtensionProperties) * vk->supported_extension_count);
  ASSERT_VK(vk->global_procs.enumerateInstanceExtensionProperties(
      NULL, &vk->supported_extension_count, vk->supported_extensions
  ));
}

bool vk_supports_instance_extension(struct vk_state *vk, char const *name) {
  for (size_t i = 0; i < vk->supported_extension_count; ++i)
    if (!strcmp(vk->supported_extensions[i].extensionName, name))
      return true;
  return false;
}

void vk_create_instance(
    struct vk_state *vk, VkInstanceCreateInfo *create_info
) {
  ASSERT_VK(vk->global_procs.createInstance(create_info, NULL, &vk->instance));

  vk->instance_procs = get_vk_instance_procs(vk->instance);
}

void vk_choose_physical_device(struct vk_state *vk) {
  uint32_t device_count = 0;
  ASSERT_VK(vk->instance_procs.enumeratePhysicalDevices(
      vk->instance, &device_count, NULL
  ));

  if (!device_count)
    panicf("No Vulkan devices present");

  VkPhysicalDevice *devices = malloc(sizeof(VkPhysicalDevice) * device_count);
  ASSERT_VK(vk->instance_procs.enumeratePhysicalDevices(
      vk->instance, &device_count, devices
  ));

  vk->physical_device = VK_NULL_HANDLE;
  // pick the first one and hope it'll work :)
  vk->physical_device = devices[0];

  free(devices);
}

void vk_choose_queue(struct vk_state *vk) {
  uint32_t num_families;
  vk->instance_procs.getPhysicalDeviceQueueFamilyProperties(
      vk->physical_device, &num_families, NULL
  );

  VkQueueFamilyProperties *queue_families =
      malloc(sizeof(VkQueueFamilyProperties) * num_families);
  vk->instance_procs.getPhysicalDeviceQueueFamilyProperties(
      vk->physical_device, &num_families, queue_families
  );

  const VkFlags required = VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT;
  for (uint32_t i = 0; i < num_families; ++i) {
    if ((queue_families[i].queueFlags & required) != required)
      continue;
    if (glfwGetPhysicalDevicePresentationSupport(
            vk->instance, vk->physical_device, i
        ) != GLFW_TRUE)
      continue;

    vk->queue_family = i;
    free(queue_families);
    return;
  }

  free(queue_families);
  panicf("no suitable queue family found");
}

void vk_create_device(
    struct vk_state *vk, sbr_vk_physical_device_features *features
) {
  VkDeviceQueueCreateInfo queue_create_info = {
      .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
      .queueFamilyIndex = vk->queue_family,
      .queueCount = 1,
      .pQueuePriorities = &(float){1.0f}
  };
  VkPhysicalDeviceFeatures physical_features = {};
  VkDeviceCreateInfo device_create_info = {
      .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
      .pQueueCreateInfos = &queue_create_info,
      .queueCreateInfoCount = 1,
      .pEnabledFeatures = &physical_features,
  };

  char const *const *sbr_extensions;
  size_t num_sbr_extensions;
  sbr_vk_physical_device_features_required_extensions(
      features, &sbr_extensions, &num_sbr_extensions
  );

  vk->device_extensions = svec_alloc(num_sbr_extensions + 1);
  svec_set_insert(&vk->device_extensions, VK_KHR_SWAPCHAIN_EXTENSION_NAME);
  for (size_t i = 0; i < num_sbr_extensions; ++i)
    svec_set_insert(&vk->device_extensions, sbr_extensions[i]);

  for (size_t i = 0; i < vk->device_extensions.len; ++i)
    printf("enabled device extension: %s\n", vk->device_extensions.data[i]);

  device_create_info.ppEnabledExtensionNames = vk->device_extensions.data;
  device_create_info.enabledExtensionCount = vk->device_extensions.len;

  sbr_vk_physical_device_features_add_to_device_create(
      features, &device_create_info
  );

  ASSERT_VK(vk->instance_procs.createDevice(
      vk->physical_device, &device_create_info, NULL, &vk->device
  ));
  vk->instance_procs.getDeviceQueue(
      vk->device, vk->queue_family, 0, &vk->queue
  );
}

typedef struct vk_swch {
  vk_state *vk;
  VkSwapchainKHR sw;
  VkImage *images;
  uint32_t num_images;
} vk_swch;

void vk_swch_init(vk_swch *swch, vk_state *vk) {
  swch->vk = vk;
  swch->sw = VK_NULL_HANDLE;
  swch->images = NULL;
}

void vk_swch_create(vk_swch *swch, VkSurfaceKHR surface, VkExtent2D extent) {
  vk_state *vk = swch->vk;

  if (swch->sw != VK_NULL_HANDLE) {
    vk->instance_procs.destroySwapchainKHR(vk->device, swch->sw, NULL);
    swch->sw = VK_NULL_HANDLE;
  }

  if (swch->images) {
    free(swch->images);
    swch->images = NULL;
  }

  VkSwapchainCreateInfoKHR swapchain_create_info = {
      .sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
      .surface = surface,
      .compositeAlpha = VK_COMPOSITE_ALPHA_PRE_MULTIPLIED_BIT_KHR,
      .imageFormat = VK_FORMAT_B8G8R8A8_UNORM,
      .imageColorSpace = VK_COLOR_SPACE_SRGB_NONLINEAR_KHR,
      .imageExtent = extent,
      .imageArrayLayers = 1,
      .imageUsage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT,
      .imageSharingMode = VK_SHARING_MODE_EXCLUSIVE,
      .presentMode = VK_PRESENT_MODE_FIFO_KHR,
      .clipped = VK_TRUE,
      .oldSwapchain = VK_NULL_HANDLE,
      .minImageCount = 2,
      .queueFamilyIndexCount = 0,
      .pQueueFamilyIndices = NULL
  };

  ASSERT_VK(vk->instance_procs.createSwapchainKHR(
      vk->device, &swapchain_create_info, NULL, &swch->sw
  ));

  vk->instance_procs.getSwapchainImagesKHR(
      vk->device, swch->sw, &swch->num_images, NULL
  );

  swch->images = malloc(sizeof(VkImage) * swch->num_images);
  vk->instance_procs.getSwapchainImagesKHR(
      vk->device, swch->sw, &swch->num_images, swch->images
  );
}

VkResult vk_swch_wait_for_next_image(vk_swch *swch, uint32_t *index) {
  vk_state *vk = swch->vk;

  return vk->instance_procs.acquireNextImageKHR(
      vk->device, swch->sw, 1000000000, VK_NULL_HANDLE, VK_NULL_HANDLE, index
  );
}

int main(int argc, char **argv) {
  if (argc != 2)
    panicf("usage: %s <file>", argc ? argv[0] : "./a.out");

  char const *input_file_path = argv[1];

  sbr_library *sbr = sbr_library_init();
  sbr_subtitles *subs = sbr_load_file(sbr, input_file_path);

  glfwInit();

  if (glfwVulkanSupported() != GLFW_TRUE)
    panicf("glfwVulkanSupported returned false");

  struct vk_state vk;

  vk_init(&vk);

  sbr_vk_entry *sbr_vk_entry =
      sbr_vk_entry_create(sbr, (void *)glfwGetInstanceProcAddress);

  const char **glfw_extensions;
  uint32_t num_glfw_extensions;
  glfw_extensions = glfwGetRequiredInstanceExtensions(&num_glfw_extensions);

  char const *const *sbr_extensions;
  size_t num_sbr_extensions;
  if (sbr_vk_entry_desired_extensions(
          sbr_vk_entry, 0, &sbr_extensions, &num_sbr_extensions
      ) < 0)
    panicf("sbr_vk_entry_desired_extensions failed!");

  stringvec instance_extensions =
      svec_alloc(num_glfw_extensions + num_sbr_extensions);

  for (size_t i = 0; i < num_glfw_extensions; ++i)
    svec_set_insert(&instance_extensions, glfw_extensions[i]);

  for (size_t i = 0; i < num_sbr_extensions; ++i)
    svec_set_insert(&instance_extensions, sbr_extensions[i]);

  VkApplicationInfo appinfo = {
      .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
      .apiVersion = VK_API_VERSION_1_2
  };

  VkInstanceCreateInfo create_info = {
      .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
      .pApplicationInfo = &appinfo,
  };

  create_info.ppEnabledExtensionNames = instance_extensions.data;
  create_info.enabledExtensionCount = instance_extensions.len;

  for (size_t i = 0; i < instance_extensions.len; ++i)
    printf("enabled instance extension: %s\n", instance_extensions.data[i]);

  vk_create_instance(&vk, &create_info);

  const uint64_t sbr_flags = 0;

  // clang-format off
  struct sbr_vk_instance_params instance_params = (struct sbr_vk_instance_params) {
    .flags = sbr_flags,
    .extensions = instance_extensions.data,
    .num_extensions = instance_extensions.len,
    .android_sdk_version = 0,
  };
  // clang-format on

  sbr_vk_instance *sbr_vk_instance =
      sbr_vk_instance_create(sbr_vk_entry, vk.instance, &instance_params);
  if (!sbr_vk_instance)
    panicf("sbr_vk_instance_create failed");

  svec_free(&instance_extensions);

  vk_choose_physical_device(&vk);

  sbr_vk_adapter *sbr_adapter =
      sbr_vk_adapter_create(sbr_vk_instance, vk.physical_device);

  sbr_vk_physical_device_features *sbr_required_features =
      sbr_vk_adapter_required_physical_device_features(sbr_adapter, sbr_flags);

  vk_choose_queue(&vk);
  vk_create_device(&vk, sbr_required_features);

  // clang-format off
  struct sbr_vk_device_params device_params = (struct sbr_vk_device_params) {
    .flags = sbr_flags,
    .enabled_extensions = NULL,
    .num_enabled_extensions = 0,
    .family_index = vk.queue_family,
    .queue_index = 0
  };
  // clang-format on

  sbr_vk_physical_device_features_required_extensions(
      sbr_required_features, &device_params.enabled_extensions,
      &device_params.num_enabled_extensions
  );
  sbr_vk_device *sbr_device =
      sbr_vk_device_from_raw(sbr_adapter, vk.device, &device_params);

  sbr_vk_rasterizer *rasterizer = sbr_vk_rasterizer_create(sbr_device);

  printf("subrandr rasterizer succesfully created! %p\n", rasterizer);

  glfwWindowHint(GLFW_CLIENT_API, GLFW_NO_API);
  glfwWindowHint(GLFW_RESIZABLE, GLFW_TRUE);
  GLFWwindow *window =
      glfwCreateWindow(800, 600, "subrandr C Vulkan example", NULL, NULL);

  VkSurfaceKHR surface;
  ASSERT_VK(glfwCreateWindowSurface(vk.instance, window, NULL, &surface));

  int width, height;
  glfwGetFramebufferSize(window, &width, &height);

  VkExtent2D extent = {.width = width, .height = height};

  vk_swch swch;

  vk_swch_init(&swch, &vk);

  // assume swapchain support is adequate :)

  vk_swch_create(&swch, surface, extent);

  printf("swapchain acquired! %p\n", swch.sw);

  sbr_renderer *renderer = sbr_renderer_create(sbr);
  sbr_renderer_set_subtitles(renderer, subs);

  bool want_new_swapchain = false;

  struct timespec start;
  if (clock_gettime(CLOCK_MONOTONIC, &start) < 0)
    panicf("clock_gettime failed");

  while (!glfwWindowShouldClose(window)) {
    uint32_t image_index;
    VkResult result = vk_swch_wait_for_next_image(&swch, &image_index);

    if (result == VK_ERROR_OUT_OF_DATE_KHR || want_new_swapchain) {
      glfwGetFramebufferSize(window, &width, &height);
      extent.width = width;
      extent.height = height;
      printf(
          "recreating swapchain for extent %u %u\n", extent.width, extent.height
      );
      if (want_new_swapchain)
        printf("swapchain was recreated because of VK_SUBOPTIMAL_KHR\n");
      vk_swch_create(&swch, surface, extent);
      want_new_swapchain = false;
      continue;
    } else {
      ASSERT_VK(result);
    }

    sbr_vk_render_target *target = sbr_vk_rasterizer_create_render_target(
        rasterizer, swch.images[image_index], &extent
    );

    sbr_subtitle_context ctx = {
        .dpi = 144,
        .video_height = extent.height << 6,
        .video_width = extent.width << 6
    };

    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) < 0)
      panicf("clock_gettime failed");

    uint32_t t = 0;
    t += (now.tv_sec - start.tv_sec) * 1000;
    t += (now.tv_nsec - start.tv_nsec) / 1000000;

    sbr_renderer_render_to(
        renderer, &ctx, t, (sbr_rasterizer *)rasterizer,
        (sbr_render_target *)target
    );

    sbr_vk_rasterizer_submit(rasterizer, target);

    VkPresentInfoKHR present_info = {
        .sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
        .pSwapchains = &swch.sw,
        .swapchainCount = 1,
        .pImageIndices = &image_index,
    };

    result = vk.instance_procs.queuePresentKHR(vk.queue, &present_info);
    if (result == VK_SUBOPTIMAL_KHR)
      want_new_swapchain = true;
    else
      ASSERT_VK(result);

    glfwPollEvents();
  }

  sbr_vk_rasterizer_destroy(rasterizer);
  sbr_vk_device_destroy(sbr_device);
  sbr_vk_adapter_destroy(sbr_adapter);
  sbr_vk_instance_destroy(sbr_vk_instance);
  sbr_vk_entry_destroy(sbr_vk_entry);

  glfwDestroyWindow(window);
  glfwTerminate();
}
