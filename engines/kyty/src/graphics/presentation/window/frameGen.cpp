// Only built with KYTY_FSR_FRAMEGEN (CMakeLists.txt).
#include "graphics/presentation/window/frameGen.h"

#include "common/logging/log.h"
#include "common/threads.h"
#include "common/timer.h"
#include "graphics/host_gpu/graphicContext.h"
#include "graphics/host_gpu/renderer/commandScheduler.h"
#include "graphics/presentation/window/frameGenLogic.h"

// After vulkanCommon.h (VK_NO_PROTOTYPES): the SDK's header then declares no Vulkan function.
#include <FidelityFX/host/backends/vk/ffx_vk.h>
#include <FidelityFX/host/ffx_frameinterpolation.h>
#include <FidelityFX/host/ffx_opticalflow.h>
#include <SDL3/SDL_video.h>
#include <array>
#include <atomic>
#include <cstdlib>
#include <fmt/format.h>
#include <string>
#include <utility>
#include <vector>

// What the SDK's Vulkan backend (ffx_vk.cpp) expects the program to provide besides shaders.
extern "C" {

// It calls these eight by their global names. Kyty links no Vulkan loader: its functions are in a
// table filled at start-up. So the names are defined here, hidden, and forward to that table.
#define KYTY_FFX_VK(result) __attribute__((visibility("hidden"))) VKAPI_ATTR result VKAPI_CALL

KYTY_FFX_VK(VkResult) vkCreateBuffer(VkDevice device, const VkBufferCreateInfo* info,
                                     const VkAllocationCallbacks* allocator, VkBuffer* buffer) {
	return VULKAN_HPP_DEFAULT_DISPATCHER.vkCreateBuffer(device, info, allocator, buffer);
}
KYTY_FFX_VK(VkResult) vkEnumerateDeviceExtensionProperties(VkPhysicalDevice device, const char* layer,
                                                           uint32_t*              count,
                                                           VkExtensionProperties* properties) {
	return VULKAN_HPP_DEFAULT_DISPATCHER.vkEnumerateDeviceExtensionProperties(device, layer, count, properties);
}
KYTY_FFX_VK(PFN_vkVoidFunction) vkGetDeviceProcAddr(VkDevice device, const char* name) {
	return VULKAN_HPP_DEFAULT_DISPATCHER.vkGetDeviceProcAddr(device, name);
}
KYTY_FFX_VK(void) vkGetPhysicalDeviceFeatures(VkPhysicalDevice device, VkPhysicalDeviceFeatures* features) {
	VULKAN_HPP_DEFAULT_DISPATCHER.vkGetPhysicalDeviceFeatures(device, features);
}
KYTY_FFX_VK(void) vkGetPhysicalDeviceFeatures2(VkPhysicalDevice device, VkPhysicalDeviceFeatures2* features) {
	VULKAN_HPP_DEFAULT_DISPATCHER.vkGetPhysicalDeviceFeatures2(device, features);
}
KYTY_FFX_VK(void) vkGetPhysicalDeviceMemoryProperties(VkPhysicalDevice                  device,
                                                      VkPhysicalDeviceMemoryProperties* properties) {
	VULKAN_HPP_DEFAULT_DISPATCHER.vkGetPhysicalDeviceMemoryProperties(device, properties);
}
KYTY_FFX_VK(void) vkGetPhysicalDeviceProperties(VkPhysicalDevice device, VkPhysicalDeviceProperties* properties) {
	VULKAN_HPP_DEFAULT_DISPATCHER.vkGetPhysicalDeviceProperties(device, properties);
}
KYTY_FFX_VK(void) vkGetPhysicalDeviceProperties2(VkPhysicalDevice device, VkPhysicalDeviceProperties2* properties) {
	VULKAN_HPP_DEFAULT_DISPATCHER.vkGetPhysicalDeviceProperties2(device, properties);
}

#undef KYTY_FFX_VK

// The backend's interface carries this entry of AMD's swapchain proxy; without the proxy nothing
// calls it.
FfxErrorCode ffxSetFrameGenerationConfigToSwapchainVK(FfxFrameGenerationConfig const* /*config*/) {
	return FFX_OK;
}

} // extern "C"

namespace Libs::Graphics::FrameGen {
namespace {

void Say(const std::string& text) {
	Log::WriteToConsoleAndLog("FSR frame generation: " + text + "\n");
}

// The camera is not known to the renderer. AMD uses these for the depth of disocclusions only.
// shortcut: fixed camera values; if generated frames show halos around moving objects, take the
// near and far planes and the field of view from the title's projection matrix.
constexpr float kCameraNear = 0.1f;
constexpr float kCameraFar  = 1000.0f;
constexpr float kCameraFov  = 1.0f; // vertical, radians

struct Slot {
	std::unique_ptr<VulkanImage> depth;
	std::unique_ptr<VulkanImage> motion;
	uint64_t                     serial = 0; // of the capture it holds, 0: none
	vk::Extent2D                 render {};
	float                        jitter[2] {};
	uint32_t                     frame         = 0;
	float                        frame_time_ms = 0.0f;
};

// What the slot images are made for.
struct Shape {
	vk::Extent2D extent {};
	vk::Format   depth  = vk::Format::eUndefined;
	vk::Format   motion = vk::Format::eUndefined;
	bool         operator==(const Shape&) const = default;
};

// The capture ring: written by the render thread, read by the present thread.
struct Ring {
	Common::Mutex                             mutex;
	std::array<Slot, kSlots>                  slots;
	GraphicContext*                           graphics = nullptr; // that made the images
	vk::Extent2D                              extent {};          // of every slot image
	vk::Format                                depth_format  = vk::Format::eUndefined;
	vk::Format                                motion_format = vk::Format::eUndefined;
	Shape                                     failed {};         // its images could not be made
	uint64_t                                  remakes       = 0; // times slot images were retired
	uint64_t                                  passed        = 0; // remakes the presenter is through with
	uint64_t                                  captured_ns   = 0;
	uint32_t                                  frame         = 0; // the title's, of the last capture
	uint32_t                                  captures      = 0; // since the last report
	uint32_t                                  refused       = 0;
	bool                                      said_refused  = false;
	bool                                      closed        = false; // the generator is gone
};

// Slot images of an earlier size that no command buffer of the guest holds any more
// (RemakeSlots). Lock order: the ring's mutex first; the scheduler may run the deferred
// operation at once, inside RemakeSlots.
struct Retired {
	struct Batch {
		uint64_t                                  remake = 0; // Ring::remakes when they were retired
		std::vector<std::unique_ptr<VulkanImage>> images;
	};
	Common::Mutex      mutex;
	std::vector<Batch> batches;
};

Ring                  g_ring;
Retired               g_retired;
std::atomic<uint64_t> g_serial {0};
// Set by the generator while it can use captures: without it the pass's images are not copied.
std::atomic<bool>     g_capturing {false};
std::atomic<uint32_t> g_refresh_mhz {0};

[[nodiscard]] vk::ImageAspectFlags Aspects(vk::Format format) {
	switch (format) {
		case vk::Format::eD32Sfloat: return vk::ImageAspectFlagBits::eDepth;
		case vk::Format::eD32SfloatS8Uint:
			return vk::ImageAspectFlagBits::eDepth | vk::ImageAspectFlagBits::eStencil;
		default: return vk::ImageAspectFlagBits::eColor;
	}
}

[[nodiscard]] bool IsDepth(vk::Format format) {
	return format == vk::Format::eD32Sfloat || format == vk::Format::eD32SfloatS8Uint;
}

// Deletes the retired images of remakes up to `passed`.
void DeleteRetired(GraphicContext& graphics, uint64_t passed) {
	Common::LockGuard lock(g_retired.mutex);
	std::erase_if(g_retired.batches, [&](Retired::Batch& batch) {
		if (batch.remake > passed) {
			return false;
		}
		for (auto& image: batch.images) {
			graphics.DeleteImage(*image);
		}
		return true;
	});
}

// Caller holds the ring's mutex, and no command buffer holds the slot images.
void DeleteSlotImages(GraphicContext& graphics) {
	for (auto& slot: g_ring.slots) {
		for (auto* image: {&slot.depth, &slot.motion}) {
			if (*image != nullptr && (*image)->image != nullptr) {
				graphics.DeleteImage(**image);
			}
			image->reset();
		}
		slot.serial = 0;
	}
	g_ring.extent = vk::Extent2D {};
}

// Caller holds the ring's mutex. New slot images for another size or format; false: no ring.
[[nodiscard]] bool RemakeSlots(CommandScheduler& guest, const CaptureInput& input) {
	auto& graphics  = guest.Graphics();
	g_ring.graphics = &graphics;
	constexpr auto needed = vk::FormatFeatureFlagBits::eSampledImage | vk::FormatFeatureFlagBits::eTransferDst;
	for (const auto format: {input.depth_format, input.motion_format}) {
		if ((graphics.GetFormatProperties(format).optimalTilingFeatures & needed) != needed) {
			return false;
		}
	}
	// The old images may be in command buffers of both threads. They go to g_retired once the
	// guest's are through the GPU, and Capture deletes them once the presenter says the same of
	// its own (Generator::Prepare).
	Retired::Batch old;
	for (auto& slot: g_ring.slots) {
		for (auto* image: {&slot.depth, &slot.motion}) {
			if (*image != nullptr && (*image)->image != nullptr) {
				old.images.push_back(std::move(*image));
			}
			image->reset();
		}
		slot.serial = 0;
	}
	g_ring.extent = vk::Extent2D {};
	if (!old.images.empty()) {
		old.remake = ++g_ring.remakes;
		guest.DeferOperation([old = std::move(old)]() mutable {
			Common::LockGuard lock(g_retired.mutex);
			g_retired.batches.push_back(std::move(old));
		});
	}
	vk::ImageCreateInfo create {};
	create.imageType     = vk::ImageType::e2D;
	create.extent        = vk::Extent3D {input.output.width, input.output.height, 1};
	create.mipLevels     = 1;
	create.arrayLayers   = 1;
	create.tiling        = vk::ImageTiling::eOptimal;
	create.initialLayout = vk::ImageLayout::eUndefined;
	create.usage         = vk::ImageUsageFlagBits::eSampled | vk::ImageUsageFlagBits::eTransferDst;
	create.sharingMode   = vk::SharingMode::eExclusive;
	create.samples       = vk::SampleCountFlagBits::e1;
	bool made            = true;
	for (auto& slot: g_ring.slots) {
		slot.depth    = std::make_unique<VulkanImage>();
		slot.motion   = std::make_unique<VulkanImage>();
		create.format = input.depth_format;
		made          = made && graphics.CreateImage(create, *slot.depth);
		create.format = input.motion_format;
		made          = made && graphics.CreateImage(create, *slot.motion);
	}
	if (!made) {
		DeleteSlotImages(graphics); // made just now: in no command buffer
		return false;
	}
	g_ring.extent        = input.output;
	g_ring.depth_format  = input.depth_format;
	g_ring.motion_format = input.motion_format;
	return true;
}

void CopyInto(vk::CommandBuffer command, vk::Image source, vk::ImageSubresourceLayers from,
              const VulkanImage& slot, vk::Extent2D size) {
	vk::ImageMemoryBarrier barrier {};
	// The slot's old contents go; an earlier read of it by the present thread is waited for.
	barrier.dstAccessMask       = vk::AccessFlagBits::eTransferWrite;
	barrier.oldLayout           = vk::ImageLayout::eUndefined;
	barrier.newLayout           = vk::ImageLayout::eTransferDstOptimal;
	barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
	barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
	barrier.image               = slot.image;
	barrier.subresourceRange    = vk::ImageSubresourceRange {Aspects(slot.format), 0, 1, 0, 1};
	command.pipelineBarrier(vk::PipelineStageFlagBits::eAllCommands, vk::PipelineStageFlagBits::eTransfer, {},
	                        0, nullptr, 0, nullptr, 1, &barrier);
	vk::ImageCopy copy {};
	copy.srcSubresource = from;
	copy.dstSubresource = vk::ImageSubresourceLayers {from.aspectMask, 0, 0, 1};
	copy.extent         = vk::Extent3D {size.width, size.height, 1};
	command.copyImage(source, vk::ImageLayout::eTransferSrcOptimal, slot.image,
	                  vk::ImageLayout::eTransferDstOptimal, 1, &copy);
	barrier.srcAccessMask = vk::AccessFlagBits::eTransferWrite;
	barrier.dstAccessMask = vk::AccessFlagBits::eShaderRead;
	barrier.oldLayout     = vk::ImageLayout::eTransferDstOptimal;
	barrier.newLayout     = vk::ImageLayout::eShaderReadOnlyOptimal;
	command.pipelineBarrier(vk::PipelineStageFlagBits::eTransfer, vk::PipelineStageFlagBits::eComputeShader, {},
	                        0, nullptr, 0, nullptr, 1, &barrier);
}

// The frame formats AMD's interpolation takes (ffx_frameinterpolation.cpp, GetFormatPrecisionGroup)
// among those a presenter frame can have.
[[nodiscard]] FfxSurfaceFormat FrameFormat(vk::Format format) {
	switch (format) {
		case vk::Format::eR8G8B8A8Unorm: return FFX_SURFACE_FORMAT_R8G8B8A8_UNORM;
		case vk::Format::eB8G8R8A8Unorm: return FFX_SURFACE_FORMAT_B8G8R8A8_UNORM;
		case vk::Format::eA2B10G10R10UnormPack32: return FFX_SURFACE_FORMAT_R10G10B10A2_UNORM;
		case vk::Format::eR16G16B16A16Sfloat: return FFX_SURFACE_FORMAT_R16G16B16A16_FLOAT;
		case vk::Format::eB10G11R11UfloatPack32: return FFX_SURFACE_FORMAT_R11G11B10_FLOAT;
		default: return FFX_SURFACE_FORMAT_UNKNOWN;
	}
}

[[nodiscard]] FfxResource Texture(vk::Image image, vk::Extent2D extent, FfxSurfaceFormat format,
                                  uint32_t usage, FfxResourceStates state) {
	FfxResourceDescription description {};
	description.type     = FFX_RESOURCE_TYPE_TEXTURE2D;
	description.format   = format;
	description.width    = extent.width;
	description.height   = extent.height;
	description.depth    = 1;
	description.mipCount = 1;
	description.flags    = FFX_RESOURCE_FLAGS_NONE;
	description.usage    = static_cast<FfxResourceUsage>(usage);
	return ffxGetResourceVK(static_cast<VkImage>(image), description, nullptr, state);
}

// The backend asks for some device functions by their extension names and calls them unchecked;
// on a Vulkan 1.1+ device those exist under the core name only.
VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL DeviceFunction(VkDevice device, const char* name) {
	auto function = VULKAN_HPP_DEFAULT_DISPATCHER.vkGetDeviceProcAddr(device, name);
	if (std::string core(name); function == nullptr && core.ends_with("KHR")) {
		core.resize(core.size() - 3);
		function = VULKAN_HPP_DEFAULT_DISPATCHER.vkGetDeviceProcAddr(device, core.c_str());
	}
	return function;
}

// AMD's two effects and what they share. About 2 MiB: the SDK's contexts have a fixed size.
struct Contexts {
	FfxInterface                 shared {};  // owns the resources both effects use
	FfxInterface                 effects {}; // optical flow and frame interpolation
	FfxUInt32                    shared_id = 0;
	FfxOpticalflowContext        flow {};
	FfxFrameInterpolationContext interpolation {};
	// Optical flow vectors and scene change; dilated depth, dilated motion, reconstructed depth.
	std::array<FfxResourceInternal, 5> resources {};
	uint32_t                           resource_count     = 0;
	bool                               shared_made        = false;
	bool                               flow_made          = false;
	bool                               interpolation_made = false;
};

struct Key {
	vk::Extent2D frame {};
	vk::Extent2D slots {};
	vk::Format   format = vk::Format::eUndefined;
	bool         hdr    = false;
	bool         operator==(const Key&) const = default;
};

} // namespace

bool DeviceSupports(vk::PhysicalDevice physical_device) {
	// What the shaders need beyond Kyty's own requirements: reading a storage image without a
	// format (the inpainting pass), the extended storage image formats, and subgroup arithmetic
	// and quad operations in compute. vulkanWindow.cpp enables the two features when the device
	// has them.
	vk::PhysicalDeviceSubgroupProperties subgroup {};
	vk::PhysicalDeviceProperties2        properties {};
	properties.pNext = &subgroup;
	physical_device.getProperties2(&properties);
	constexpr auto operations = vk::SubgroupFeatureFlagBits::eArithmetic | vk::SubgroupFeatureFlagBits::eQuad;
	const auto     features   = physical_device.getFeatures();
	return features.shaderStorageImageReadWithoutFormat == VK_TRUE &&
	       features.shaderStorageImageExtendedFormats == VK_TRUE &&
	       static_cast<bool>(subgroup.supportedStages & vk::ShaderStageFlagBits::eCompute) &&
	       (subgroup.supportedOperations & operations) == operations && subgroup.quadOperationsInAllStages == VK_TRUE;
}

void Shutdown() {
	Common::LockGuard lock(g_ring.mutex);
	g_ring.closed = true;
	if (g_ring.graphics != nullptr) {
		DeleteSlotImages(*g_ring.graphics);
		DeleteRetired(*g_ring.graphics, g_ring.remakes);
	}
}

void NoteDisplay(SDL_Window* window) {
	// shortcut: read once, when the window is made; a window moved to a display with another
	// refresh rate keeps the first value. Call this from window.cpp's handler of
	// SDL_EVENT_WINDOW_DISPLAY_CHANGED (main thread) when that matters.
	const auto* mode = SDL_GetCurrentDisplayMode(SDL_GetDisplayForWindow(window));
	const auto  mhz  = mode != nullptr && mode->refresh_rate > 0.0f
	                       ? static_cast<uint32_t>(mode->refresh_rate * 1000.0f + 0.5f)
	                       : 0u;
	g_refresh_mhz.store(mhz, std::memory_order_relaxed);
	if (Requested()) {
		Say(mhz != 0 ? fmt::format("display refresh {:.2f} Hz", mhz / 1000.0) : std::string("display refresh unknown"));
	}
}

uint64_t LastCapture() {
	return g_serial.load(std::memory_order_acquire);
}

bool Capturing() {
	return g_capturing.load(std::memory_order_relaxed);
}

void Capture(CommandScheduler& guest, vk::CommandBuffer command, const CaptureInput& input) {
	Common::LockGuard lock(g_ring.mutex);
	if (g_ring.closed) {
		return;
	}
	DeleteRetired(guest.Graphics(), g_ring.passed);
	const bool fits = IsDepth(input.depth_format) && !IsDepth(input.motion_format) &&
	                  ffxGetSurfaceFormatVK(static_cast<VkFormat>(input.motion_format)) != FFX_SURFACE_FORMAT_UNKNOWN &&
	                  input.render.width != 0 && input.render.height != 0 &&
	                  input.render.width <= input.output.width && input.render.height <= input.output.height;
	const bool same = g_ring.extent == input.output && g_ring.depth_format == input.depth_format &&
	                  g_ring.motion_format == input.motion_format;
	bool usable = fits && same;
	// Not remade: for a shape whose images could not be made (until the shape changes), and for a
	// second shape within one title frame, which would remake the ring at every draw.
	// shortcut: a failed shape is not tried again when video memory frees up; retry after some
	// seconds if a title is seen to recover.
	if (const Shape shape {input.output, input.depth_format, input.motion_format};
	    fits && !same && shape != g_ring.failed && !(g_ring.captured_ns != 0 && input.frame == g_ring.frame)) {
		usable        = RemakeSlots(guest, input);
		g_ring.failed = usable ? Shape {} : shape;
	}
	if (!usable) {
		g_ring.refused++;
		if (!std::exchange(g_ring.said_refused, true)) {
			Say(fmt::format("the pass's inputs are not taken: depth {}, motion {}, {}x{} -> {}x{}",
			                vk::to_string(input.depth_format), vk::to_string(input.motion_format),
			                input.render.width, input.render.height, input.output.width, input.output.height));
		}
		return;
	}
	const auto serial = g_serial.load(std::memory_order_relaxed) + 1;
	auto&      slot   = g_ring.slots[SlotOf(serial)];
	CopyInto(command, input.depth, vk::ImageSubresourceLayers {vk::ImageAspectFlagBits::eDepth, 0, input.depth_layer, 1},
	         *slot.depth, input.render);
	CopyInto(command, input.motion,
	         vk::ImageSubresourceLayers {vk::ImageAspectFlagBits::eColor, input.motion_level, input.motion_layer, 1},
	         *slot.motion, input.render);
	const auto now     = Common::SteadyNs();
	slot.serial        = serial;
	slot.render        = input.render;
	slot.jitter[0]     = input.jitter[0];
	slot.jitter[1]     = input.jitter[1];
	slot.frame         = input.frame;
	slot.frame_time_ms = FrameTimeMs(g_ring.captured_ns, now);
	g_ring.captured_ns = now;
	g_ring.frame       = input.frame;
	g_ring.captures++;
	g_serial.store(serial, std::memory_order_release);
}

struct Generator::Impl {
	Impl(GraphicContext& graphics_, CommandScheduler& scheduler_): graphics(graphics_), scheduler(scheduler_) {}

	[[nodiscard]] bool Create(const Key& wanted);
	void               Destroy();
	void               Report(uint64_t now, vk::PresentModeKHR mode);
	[[nodiscard]] bool Refuse(Outcome outcome) {
		stats.Add(outcome);
		continuity.Break();
		return false;
	}

	GraphicContext&           graphics;
	CommandScheduler&         scheduler;
	std::unique_ptr<Contexts> contexts;
	Key                       key;
	Pacer                     pacer;
	Continuity                continuity;
	Stats                     stats;
	vk::Extent2D              render {};       // of the last capture used, for the report
	uint64_t                  capture  = 0;    // of the frame Prepare saw last
	uint64_t                  previous = 0;    // of the frame before it
	uint64_t                  reported_ns = 0;
	uint32_t                  errors      = 0; // AMD errors in a row
	Outcome                   gate        = Outcome::Count; // the last gate decision logged
	bool                      features    = false;
	bool                      disabled    = false;
	bool                      warned_rate = false;
};

bool Generator::Impl::Create(const Key& wanted) {
	const auto started = Common::SteadyNs();
	contexts           = std::make_unique<Contexts>();
	auto& c            = *contexts;
	key                = wanted;
	const auto physical = static_cast<VkPhysicalDevice>(graphics.physical_device);
	VkDeviceContext device {static_cast<VkDevice>(graphics.device), physical, DeviceFunction};
	const auto      ffx_device = ffxGetDeviceVK(&device); // copied by the SDK
	const auto      backend    = [&](FfxInterface& out, size_t count) -> FfxErrorCode {
        const auto size    = ffxGetScratchMemorySizeVK(physical, count);
        auto*      scratch = std::calloc(1, size);
        if (scratch == nullptr) {
            return FFX_ERROR_OUT_OF_MEMORY;
        }
        const auto error = ffxGetInterfaceVK(&out, ffx_device, scratch, size, count);
        if (error != FFX_OK) {
            std::free(scratch);
            out.scratchBuffer = nullptr;
        }
        return error;
	};
	const FfxDimensions2D display {wanted.frame.width, wanted.frame.height};
	const auto            format = FrameFormat(wanted.format);
	const auto&           options = Embed::GetOptions();
	const char*           step    = "the shared backend";
	FfxErrorCode          error   = backend(c.shared, 1);
	if (error == FFX_OK) {
		error = backend(c.effects, 2);
	}
	if (error == FFX_OK) {
		error         = c.shared.fpCreateBackendContext(&c.shared, FFX_EFFECT_SHAREDAPIBACKEND, nullptr, &c.shared_id);
		c.shared_made = error == FFX_OK;
	}
	if (error == FFX_OK) {
		step = "optical flow";
		FfxOpticalflowContextDescription description {};
		description.backendInterface = c.effects;
		description.resolution       = display;
		error                        = ffxOpticalflowContextCreate(&c.flow, &description);
		c.flow_made                  = error == FFX_OK;
	}
	if (error == FFX_OK) {
		step = "frame interpolation";
		FfxFrameInterpolationContextDescription description {};
		// The title's motion vectors are at render size and include the jitter (upscalePass.h).
		description.flags = FFX_FRAMEINTERPOLATION_ENABLE_JITTER_MOTION_VECTORS |
		                    (options.dlss_reverse_depth ? FFX_FRAMEINTERPOLATION_ENABLE_DEPTH_INVERTED : 0) |
		                    (wanted.hdr ? FFX_FRAMEINTERPOLATION_ENABLE_HDR_COLOR_INPUT : 0);
		description.maxRenderSize                     = {wanted.slots.width, wanted.slots.height};
		description.displaySize                       = display;
		description.backBufferFormat                  = format;
		description.previousInterpolationSourceFormat = format;
		description.backendInterface                  = c.effects;
		error                = ffxFrameInterpolationContextCreate(&c.interpolation, &description);
		c.interpolation_made = error == FFX_OK;
	}
	FfxOpticalflowSharedResourceDescriptions        flow_shared {};
	FfxFrameInterpolationSharedResourceDescriptions interpolation_shared {};
	if (error == FFX_OK) {
		step  = "the shared resources";
		error = ffxOpticalflowGetSharedResourceDescriptions(&c.flow, &flow_shared);
	}
	if (error == FFX_OK) {
		error = ffxFrameInterpolationGetSharedResourceDescriptions(&c.interpolation, &interpolation_shared);
	}
	// In the order of Contexts::resources.
	for (const auto* description: {&flow_shared.opticalFlowVector, &flow_shared.opticalFlowSCD,
	                               &interpolation_shared.dilatedDepth, &interpolation_shared.dilatedMotionVectors,
	                               &interpolation_shared.reconstructedPrevNearestDepth}) {
		if (error == FFX_OK) {
			error = c.shared.fpCreateResource(&c.shared, description, c.shared_id, &c.resources[c.resource_count]);
			c.resource_count += error == FFX_OK ? 1 : 0;
		}
	}
	if (error != FFX_OK) {
		Say(fmt::format("creating {} failed with AMD error 0x{:08x}; off for this run", step,
		                static_cast<uint32_t>(error)));
		Destroy();
		return false;
	}
	FfxEffectMemoryUsage flow_memory {};
	FfxEffectMemoryUsage interpolation_memory {};
	FfxEffectMemoryUsage shared_memory {};
	(void)ffxOpticalflowContextGetGpuMemoryUsage(&c.flow, &flow_memory);
	(void)ffxFrameInterpolationContextGetGpuMemoryUsage(&c.interpolation, &interpolation_memory);
	(void)ffxSharedContextGetGpuMemoryUsage(&c.shared, &shared_memory);
	Say(fmt::format("contexts for frames of {}x{} {}{}, render sizes up to {}x{}, depth {}: made in {} ms, {} MiB of "
	                "video memory",
	                wanted.frame.width, wanted.frame.height, vk::to_string(wanted.format), wanted.hdr ? " (HDR10)" : "",
	                wanted.slots.width, wanted.slots.height, options.dlss_reverse_depth ? "reversed" : "forward",
	                (Common::SteadyNs() - started) / 1000000,
	                (flow_memory.totalUsageInBytes + interpolation_memory.totalUsageInBytes +
	                 shared_memory.totalUsageInBytes) >> 20u));
	return true;
}

void Generator::Impl::Destroy() {
	if (contexts == nullptr) {
		return;
	}
	// Everything recorded with these contexts has to be through the GPU.
	scheduler.Wait(scheduler.CurrentTick() - 1);
	auto& c = *contexts;
	for (uint32_t i = 0; i < c.resource_count; i++) {
		(void)c.shared.fpDestroyResource(&c.shared, c.resources[i], c.shared_id);
	}
	if (c.interpolation_made) {
		(void)ffxFrameInterpolationContextDestroy(&c.interpolation);
	}
	if (c.flow_made) {
		(void)ffxOpticalflowContextDestroy(&c.flow);
	}
	if (c.shared_made) {
		(void)c.shared.fpDestroyBackendContext(&c.shared, c.shared_id);
	}
	std::free(c.effects.scratchBuffer);
	std::free(c.shared.scratchBuffer);
	contexts.reset();
}

// One line per ten seconds while frames come.
void Generator::Impl::Report(uint64_t now, vk::PresentModeKHR mode) {
	if (now - reported_ns < 10'000'000'000ull) {
		return;
	}
	reported_ns = now;
	auto text   = stats.Take();
	if (text.empty()) {
		return;
	}
	uint32_t captures = 0;
	uint32_t refused  = 0;
	{
		Common::LockGuard lock(g_ring.mutex);
		captures = std::exchange(g_ring.captures, 0);
		refused  = std::exchange(g_ring.refused, 0);
	}
	uint64_t memory = 0;
	if (contexts != nullptr) {
		FfxEffectMemoryUsage usage {};
		if (ffxOpticalflowContextGetGpuMemoryUsage(&contexts->flow, &usage) == FFX_OK) {
			memory += usage.totalUsageInBytes;
		}
		if (ffxFrameInterpolationContextGetGpuMemoryUsage(&contexts->interpolation, &usage) == FFX_OK) {
			memory += usage.totalUsageInBytes;
		}
		if (ffxSharedContextGetGpuMemoryUsage(&contexts->shared, &usage) == FFX_OK) {
			memory += usage.totalUsageInBytes;
		}
	}
	Say(fmt::format("{}; captures {}, refused {}; render {}x{} -> frame {}x{}; half interval {:.1f} ms; {}, "
	                "refresh {:.2f} Hz; AMD video memory {} MiB of a budget of {} MiB",
	                text, captures, refused, render.width, render.height, key.frame.width, key.frame.height,
	                static_cast<double>(pacer.HalfNs()) / 1.0e6, vk::to_string(mode),
	                g_refresh_mhz.load(std::memory_order_relaxed) / 1000.0, memory >> 20u,
	                graphics.GetTotalMemoryBudget() >> 20u));
}

Generator::Generator(GraphicContext& graphics, CommandScheduler& scheduler)
    : m_impl(std::make_unique<Impl>(graphics, scheduler)) {
	m_impl->features = DeviceSupports(graphics.physical_device);
	Say(fmt::format("requested; AMD FidelityFX SDK 1.1.4, frame interpolation {}.{}.{}; device features {}",
	                FFX_FRAMEINTERPOLATION_VERSION_MAJOR, FFX_FRAMEINTERPOLATION_VERSION_MINOR,
	                FFX_FRAMEINTERPOLATION_VERSION_PATCH, m_impl->features ? "present" : "missing"));
}

Generator::~Generator() {
	g_capturing.store(false, std::memory_order_relaxed);
	m_impl->Destroy();
	// The slot images stay: command buffers of the guest may still hold them (Shutdown).
	Common::LockGuard lock(g_ring.mutex);
	g_ring.closed = true;
}

bool Generator::Prepare(const Source& frame, vk::PresentModeKHR mode) {
	auto&      s   = *m_impl;
	const auto now = Common::SteadyNs();
	s.pacer.Note(now);
	s.Report(now, mode);
	// Slot images retired since the last frame: what this thread recorded with them was submitted
	// (the caller submits right after Record), so once that is through the GPU Capture may delete them.
	uint64_t remakes = 0;
	uint64_t passed  = 0;
	{
		Common::LockGuard lock(g_ring.mutex);
		remakes = g_ring.remakes;
		passed  = g_ring.passed;
	}
	if (passed != remakes) {
		s.scheduler.Wait(s.scheduler.CurrentTick() - 1);
		Common::LockGuard lock(g_ring.mutex);
		g_ring.passed = remakes;
	}
	if (frame.blank) {
		return s.Refuse(Outcome::Blank);
	}
	s.previous         = std::exchange(s.capture, frame.capture);
	const bool fifo    = mode == vk::PresentModeKHR::eFifo;
	const auto refresh = g_refresh_mhz.load(std::memory_order_relaxed);
	const auto limit   = Embed::FrameLimit();
	auto       gate    = Outcome::Generated;
	if (s.disabled) {
		gate = Outcome::Error;
	} else if (!s.features || FrameFormat(frame.format) == FFX_SURFACE_FORMAT_UNKNOWN ||
	           !(s.graphics.GetFormatProperties(frame.format).optimalTilingFeatures &
	             vk::FormatFeatureFlagBits::eStorageImage)) {
		gate = Outcome::Format;
	} else if (!DisplayAllows(fifo, refresh, limit)) {
		gate = Outcome::Display;
	}
	if (gate != s.gate) {
		s.gate = gate;
		switch (gate) {
			case Outcome::Generated:
				Say(fmt::format("on: {}, refresh {:.2f} Hz, game limit {} fps", vk::to_string(mode),
				                refresh / 1000.0, limit));
				break;
			case Outcome::Display:
				Say(fmt::format("off: with vertical sync ({}) the display has to refresh at least twice per "
				                "game frame; refresh {}, game limit {} fps",
				                vk::to_string(mode),
				                refresh != 0 ? fmt::format("{:.2f} Hz", refresh / 1000.0) : std::string("unknown"),
				                limit));
				break;
			case Outcome::Format:
				Say(fmt::format("off: frames of format {} or this device's features are not taken",
				                vk::to_string(frame.format)));
				break;
			default: Say("off after errors"); break;
		}
	}
	g_capturing.store(gate == Outcome::Generated, std::memory_order_relaxed);
	if (gate != Outcome::Generated) {
		return s.Refuse(gate);
	}
	if (!fifo && refresh != 0 && !DisplayAllows(true, refresh, limit) && !std::exchange(s.warned_rate, true)) {
		Say("the display refreshes less than twice per game frame: not every frame can be shown");
	}
	// The slot's own serial is checked when recording.
	if (Find(frame.capture, s.previous, frame.capture) == Lookup::NoInputs) {
		return s.Refuse(Outcome::NoInputs);
	}
	Key key {frame.extent, {}, frame.format, frame.hdr};
	{
		Common::LockGuard lock(g_ring.mutex);
		key.slots = g_ring.extent;
	}
	if (key.slots.width == 0) {
		return s.Refuse(Outcome::Lost); // the ring was remade, or not made
	}
	if (s.contexts == nullptr || key != s.key) {
		s.Destroy();
		s.continuity.Break();
		if (!s.Create(key)) {
			s.disabled = true;
			return s.Refuse(Outcome::Error);
		}
	}
	return true;
}

bool Generator::Record(vk::CommandBuffer command, const Source& frame, vk::Image output) {
	auto&             s = *m_impl;
	auto&             c = *s.contexts;
	Common::LockGuard lock(g_ring.mutex);
	const auto&       slot = g_ring.slots[SlotOf(frame.capture)];
	if (g_ring.extent != s.key.slots || Find(frame.capture, s.previous, slot.serial) != Lookup::Found) {
		return s.Refuse(Outcome::Lost);
	}
	s.render          = slot.render;
	const auto step   = s.continuity.Next(slot.frame);
	const auto list   = ffxGetCommandListVK(static_cast<VkCommandBuffer>(command));
	const auto shared = [&](size_t index) { return c.shared.fpGetResource(&c.shared, c.resources[index]); };
	const FfxDimensions2D render {slot.render.width, slot.render.height};
	const FfxDimensions2D display {frame.extent.width, frame.extent.height};
	// AMD views a depth image by the usage: both aspects named for a depth-stencil format.
	const uint32_t depth_usage = FFX_RESOURCE_USAGE_DEPTHTARGET |
	                             (g_ring.depth_format == vk::Format::eD32SfloatS8Uint ? FFX_RESOURCE_USAGE_STENCILTARGET : 0);
	const auto color = Texture(frame.image, frame.extent, FrameFormat(frame.format), FFX_RESOURCE_USAGE_READ_ONLY,
	                           FFX_RESOURCE_STATE_COMPUTE_READ);
	const auto transfer = frame.hdr ? FFX_BACKBUFFER_TRANSFER_FUNCTION_PQ : FFX_BACKBUFFER_TRANSFER_FUNCTION_SRGB;
	// The luminance range of the frame's values, in nits for HDR10.
	const float luminance[2] = {0.0f, frame.hdr ? 1000.0f : 1.0f};

	FfxFrameInterpolationPrepareDescription prepare {};
	prepare.commandList = list;
	prepare.renderSize  = render;
	prepare.jitterOffset = {slot.jitter[0], slot.jitter[1]};
	// The title's motion vectors are in UV and point from the previous to the current position.
	prepare.motionVectorScale       = {-static_cast<float>(render.width), -static_cast<float>(render.height)};
	prepare.frameTimeDelta          = slot.frame_time_ms;
	prepare.cameraNear              = kCameraNear;
	prepare.cameraFar               = kCameraFar;
	prepare.viewSpaceToMetersFactor = 1.0f;
	prepare.cameraFovAngleVertical  = kCameraFov;
	prepare.depth = Texture(slot.depth->image, g_ring.extent, FFX_SURFACE_FORMAT_R32_FLOAT, depth_usage,
	                        FFX_RESOURCE_STATE_COMPUTE_READ);
	prepare.motionVectors =
	    Texture(slot.motion->image, g_ring.extent, ffxGetSurfaceFormatVK(static_cast<VkFormat>(g_ring.motion_format)),
	            FFX_RESOURCE_USAGE_READ_ONLY, FFX_RESOURCE_STATE_COMPUTE_READ);
	prepare.frameID                = step.frame_id;
	prepare.dilatedDepth           = shared(2);
	prepare.dilatedMotionVectors   = shared(3);
	prepare.reconstructedPrevDepth = shared(4);
	auto error                     = ffxFrameInterpolationPrepare(&c.interpolation, &prepare);

	if (error == FFX_OK) {
		FfxOpticalflowDispatchDescription flow {};
		flow.commandList                = list;
		flow.color                      = color;
		flow.opticalFlowVector          = shared(0);
		flow.opticalFlowSCD             = shared(1);
		flow.reset                      = step.reset;
		flow.backbufferTransferFunction = transfer;
		flow.minMaxLuminance            = {luminance[0], luminance[1]};
		error                           = ffxOpticalflowContextDispatch(&c.flow, &flow);
	}
	if (error == FFX_OK) {
		FfxFrameInterpolationDispatchDescription dispatch {};
		dispatch.commandList       = list;
		dispatch.displaySize       = display;
		dispatch.renderSize        = render;
		dispatch.currentBackBuffer = color;
		dispatch.output = Texture(output, frame.extent, FrameFormat(frame.format), FFX_RESOURCE_USAGE_UAV,
		                          FFX_RESOURCE_STATE_UNORDERED_ACCESS);
		dispatch.interpolationRect               = {0, 0, static_cast<int32_t>(display.width),
		                                            static_cast<int32_t>(display.height)};
		dispatch.opticalFlowVector               = shared(0);
		dispatch.opticalFlowSceneChangeDetection = shared(1);
		dispatch.opticalFlowBlockSize            = 8;
		dispatch.opticalFlowScale                = {1.0f / static_cast<float>(display.width),
		                                            1.0f / static_cast<float>(display.height)};
		dispatch.cameraNear                      = kCameraNear;
		dispatch.cameraFar                       = kCameraFar;
		dispatch.cameraFovAngleVertical          = kCameraFov;
		dispatch.viewSpaceToMetersFactor         = 1.0f;
		dispatch.frameTimeDelta                  = slot.frame_time_ms;
		dispatch.reset                           = step.reset;
		dispatch.backBufferTransferFunction      = transfer;
		dispatch.minMaxLuminance[0]              = luminance[0];
		dispatch.minMaxLuminance[1]              = luminance[1];
		dispatch.frameID                         = step.frame_id;
		dispatch.dilatedDepth                    = shared(2);
		dispatch.dilatedMotionVectors            = shared(3);
		dispatch.reconstructedPrevDepth          = shared(4);
		error                                    = ffxFrameInterpolationDispatch(&c.interpolation, &dispatch);
	}
	if (error != FFX_OK) {
		if (++s.errors >= 3) {
			s.disabled = true;
		}
		if (s.errors == 1 || s.disabled) {
			Say(fmt::format("AMD error 0x{:08x} while recording{}", static_cast<uint32_t>(error),
			                s.disabled ? "; off for this run" : ""));
		}
		return s.Refuse(Outcome::Error);
	}
	s.errors = 0;
	// A reset dispatch only stores this frame as the history of the next one.
	s.stats.Add(step.reset ? Outcome::Primed : Outcome::Generated);
	return !step.reset;
}

void Generator::Pace(vk::PresentModeKHR mode, uint64_t shown_ns) {
	if (mode == vk::PresentModeKHR::eFifo) {
		return; // each present waits for its refresh
	}
	// shortcut: plain sleep, no spin and no variance margin; if judder shows, clock_nanosleep(TIMER_ABSTIME)
	// to 200 us before target + short spin, target avg/2 - 0.1*variance - 0.1 ms.
	// shortcut: the real frame is presented from inside the same flip after a sleep; if guest vblank
	// timing suffers, split into a pending second present completed by PresentThread's polling loop.
	const auto due = shown_ns + m_impl->pacer.HalfNs();
	if (const auto now = Common::SteadyNs(); due > now) {
		Common::Thread::SleepMicro(static_cast<uint32_t>((due - now) / 1000));
	}
}

} // namespace Libs::Graphics::FrameGen
