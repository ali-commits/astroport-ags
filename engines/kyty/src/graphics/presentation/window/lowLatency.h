#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_LOWLATENCY_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_LOWLATENCY_H_

#include "graphics/host_gpu/vulkanCommon.h"

#include <cstdint>
#include <mutex>
#include <vector>

// NVIDIA low-latency mode (VK_NV_low_latency2): on when the embedding host asks for it
// (Embed::Options::low_latency) and the device offers the extension and VK_KHR_present_id.
// The driver delays the start of the next guest frame (GuestSubmitted) so that it finishes just
// in time; the minimum frame interval is Embed::Options::low_latency_interval_us, followed live.
// Frames are counted by guest flips in three counters that advance in the same order: guest
// thread, flip reservation, present. Every function is a no-op while the mode is off.
namespace Libs::Graphics::LowLatency {

// Device creation: adds the two extensions when both are offered, logs "unavailable" otherwise.
void AppendExtensions(std::vector<const char*>&                   extensions,
                      const std::vector<vk::ExtensionProperties>& available);
// After the device dispatcher is initialised.
void Init(vk::Device device);
[[nodiscard]] bool Active();

// Swapchain::Create chains `latency` into `info` before creation and reports the handle after.
void ChainCreateInfo(vk::SwapchainCreateInfoKHR& info, VkSwapchainLatencyCreateInfoNV& latency);
void SwapchainCreated(vk::SwapchainKHR swapchain);
void SwapchainDestroying();
// vkQueuePresentKHR; a frame not presented before carries its number as the present id.
[[nodiscard]] vk::Result Present(vk::Queue queue, vk::PresentInfoKHR& present);
// Held around each short vkAcquireNextImageKHR wait: the guest thread's latency markers name the swapchain, which
// acquire uses externally synchronised (validation: Threading-MultipleThreads). Empty while off.
[[nodiscard]] std::unique_lock<std::mutex> SwapchainLock();

// Guest thread: a flip was written (a flip packet in a command buffer, or a CPU flip).
void FlipWritten();
// Guest thread: the work holding the flips written so far was handed to the renderer. Ends the
// guest frame and sleeps until the driver lets the next one start, at most 100 ms.
void GuestSubmitted();
// A flip request was reserved: its frame number, 0 while the mode is off.
[[nodiscard]] uint64_t RenderSubmitted();
// Present thread: the frame the next present shows.
void PresentFrame(uint64_t frame);

} // namespace Libs::Graphics::LowLatency

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_LOWLATENCY_H_
