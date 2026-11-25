#include "graphics/presentation/window/lowLatency.h"

#include "embed/embedOptions.h"
#include "graphics/presentation/window/frameGen.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <mutex>

namespace Libs::Graphics::LowLatency {

namespace {

constexpr uint64_t SLEEP_LIMIT_NS = 100'000'000;

bool     g_requested = false; // the device was created with both extensions
bool     g_active    = false;
VkDevice g_device    = nullptr;
// Never destroyed: the mode exists only in the embedded renderer, which is flushed, not torn down.
VkSemaphore g_semaphore = nullptr;

// Held around every call that names the swapchain, also across vkQueuePresentKHR.
std::mutex     g_mutex;
VkSwapchainKHR g_swapchain       = nullptr;
uint32_t       g_interval_us     = 0;
uint64_t       g_semaphore_value = 0;
uint64_t       g_render_frame    = 0;
uint64_t       g_presented       = 0;
uint64_t       g_sleeps = 0, g_timeouts = 0;
std::chrono::steady_clock::time_point g_report_at;

std::atomic<uint32_t> g_flips_written {0};
std::atomic<uint64_t> g_present_frame {0};
std::mutex            g_guest_mutex; // one guest frame ends at a time
uint64_t              g_guest_frame = 0;

const auto& Vk() {
	return VULKAN_HPP_DEFAULT_DISPATCHER;
}

void Log(const char* text) {
	std::printf("LowLatency: %s\n", text);
	std::fflush(stdout);
}

// Caller holds g_mutex.
void Marker(uint64_t frame, VkLatencyMarkerNV marker) {
	if (g_swapchain != nullptr) {
		VkSetLatencyMarkerInfoNV info {VK_STRUCTURE_TYPE_SET_LATENCY_MARKER_INFO_NV};
		info.presentID = frame;
		info.marker    = marker;
		Vk().vkSetLatencyMarkerNV(g_device, g_swapchain, &info);
	}
}

// Caller holds g_mutex. Low latency on, boost on; the driver also paces frames to the interval
// before the game starts the next one, where it lowers latency instead of after the simulation.
void SetSleepMode(uint32_t interval_us) {
	VkLatencySleepModeInfoNV mode {VK_STRUCTURE_TYPE_LATENCY_SLEEP_MODE_INFO_NV};
	mode.lowLatencyMode    = VK_TRUE;
	mode.lowLatencyBoost   = VK_TRUE;
	mode.minimumIntervalUs = interval_us;
	const auto result = Vk().vkSetLatencySleepModeNV(g_device, g_swapchain, &mode);
	g_interval_us     = interval_us;
	std::printf("LowLatency: sleep mode on (boost), minimum interval %u us -> %s\n", interval_us,
	            vk::to_string(static_cast<vk::Result>(result)).c_str());
	std::fflush(stdout);
}

// Caller holds g_mutex. Filled driver time stamps are the evidence that it acts on the markers.
void Report() {
	const auto now = std::chrono::steady_clock::now();
	if (now < g_report_at || g_swapchain == nullptr) {
		return;
	}
	g_report_at = now + std::chrono::seconds(10);
	std::array<VkLatencyTimingsFrameReportNV, 64> timings {};
	for (auto& timing: timings) {
		timing.sType = VK_STRUCTURE_TYPE_LATENCY_TIMINGS_FRAME_REPORT_NV;
	}
	VkGetLatencyMarkerInfoNV info {VK_STRUCTURE_TYPE_GET_LATENCY_MARKER_INFO_NV};
	info.timingCount = static_cast<uint32_t>(timings.size());
	info.pTimings    = timings.data();
	Vk().vkGetLatencyTimingsNV(g_device, g_swapchain, &info);
	const auto count  = std::min<size_t>(info.timingCount, timings.size());
	uint32_t   filled = 0;
	uint64_t   total  = 0;
	for (size_t i = 0; i < count; i++) {
		if (const auto& t = timings[i]; t.simStartTimeUs != 0 && t.gpuRenderEndTimeUs > t.simStartTimeUs) {
			filled++;
			total += t.gpuRenderEndTimeUs - t.simStartTimeUs;
		}
	}
	std::printf("LowLatency: %u of %zu driver reports filled, simulation start to GPU end %.1f ms; "
	            "sleeps %llu, time-outs %llu, interval %u us\n",
	            filled, count, filled != 0 ? static_cast<double>(total) / filled / 1000.0 : 0.0,
	            static_cast<unsigned long long>(g_sleeps), static_cast<unsigned long long>(g_timeouts),
	            g_interval_us);
	std::fflush(stdout);
}

} // namespace

void AppendExtensions(std::vector<const char*>&                   extensions,
                      const std::vector<vk::ExtensionProperties>& available,
                      vk::PhysicalDevice                          physical_device) {
	const auto& options = Embed::GetOptions();
	if (!options.low_latency) {
		return;
	}
	if (FrameGen::Requested() && FrameGen::DeviceSupports(physical_device)) {
		Log("off while frame generation is requested");
		return;
	}
	const auto has = [&](const char* name) {
		return std::any_of(available.begin(), available.end(),
		                   [name](const auto& e) { return std::strcmp(e.extensionName, name) == 0; });
	};
	if (!has(VK_NV_LOW_LATENCY_2_EXTENSION_NAME) || !has(VK_KHR_PRESENT_ID_EXTENSION_NAME)) {
		Log("unavailable (the device lacks VK_NV_low_latency2 or VK_KHR_present_id)");
		return;
	}
	for (const char* name: {VK_NV_LOW_LATENCY_2_EXTENSION_NAME, VK_KHR_PRESENT_ID_EXTENSION_NAME}) {
		if (std::none_of(extensions.begin(), extensions.end(),
		                 [name](const char* e) { return std::strcmp(e, name) == 0; })) {
			extensions.push_back(name);
		}
	}
	g_requested = true;
}

void Init(vk::Device device) {
	if (!g_requested) {
		return;
	}
	VkSemaphoreTypeCreateInfo type {VK_STRUCTURE_TYPE_SEMAPHORE_TYPE_CREATE_INFO};
	type.semaphoreType = VK_SEMAPHORE_TYPE_TIMELINE;
	VkSemaphoreCreateInfo info {VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO};
	info.pNext = &type;
	if (Vk().vkSetLatencySleepModeNV == nullptr || Vk().vkLatencySleepNV == nullptr ||
	    Vk().vkSetLatencyMarkerNV == nullptr || Vk().vkGetLatencyTimingsNV == nullptr ||
	    Vk().vkCreateSemaphore(device, &info, nullptr, &g_semaphore) != VK_SUCCESS) {
		Log("unavailable (the driver's entry points or the semaphore are missing)");
		return;
	}
	g_device    = device;
	g_report_at = std::chrono::steady_clock::now() + std::chrono::seconds(10);
	g_active    = true;
	Log("on (VK_NV_low_latency2)");
}

bool Active() {
	return g_active;
}

void ChainCreateInfo(vk::SwapchainCreateInfoKHR& info, VkSwapchainLatencyCreateInfoNV& latency) {
	if (g_active) {
		latency                   = {VK_STRUCTURE_TYPE_SWAPCHAIN_LATENCY_CREATE_INFO_NV};
		latency.pNext             = info.pNext;
		latency.latencyModeEnable = VK_TRUE;
		info.pNext                = &latency;
	}
}

void SwapchainCreated(vk::SwapchainKHR swapchain) {
	if (g_active) {
		std::lock_guard lock(g_mutex);
		g_swapchain = swapchain;
		SetSleepMode(Embed::GetOptions().low_latency_interval_us.load(std::memory_order_relaxed));
	}
}

void SwapchainDestroying() {
	if (g_active) {
		std::lock_guard lock(g_mutex);
		g_swapchain = nullptr;
	}
}

vk::Result Present(vk::Queue queue, vk::PresentInfoKHR& present) {
	if (!g_active) {
		return queue.presentKHR(&present);
	}
	std::lock_guard lock(g_mutex);
	const uint64_t  frame = g_present_frame.load(std::memory_order_relaxed);
	// A repeat present without a new frame carries no id.
	const bool     fresh = frame > g_presented && g_swapchain != nullptr;
	VkPresentIdKHR id {VK_STRUCTURE_TYPE_PRESENT_ID_KHR};
	const void*    next = present.pNext;
	if (fresh) {
		id.pNext          = next;
		id.swapchainCount = 1;
		id.pPresentIds    = &frame;
		present.pNext     = &id;
		Marker(frame, VK_LATENCY_MARKER_PRESENT_START_NV);
	}
	const auto result = queue.presentKHR(&present);
	if (fresh) {
		Marker(frame, VK_LATENCY_MARKER_PRESENT_END_NV);
		g_presented   = frame;
		present.pNext = next;
	}
	return result;
}

std::unique_lock<std::mutex> SwapchainLock() {
	return g_active ? std::unique_lock(g_mutex) : std::unique_lock<std::mutex>();
}

void FlipWritten() {
	if (g_active) {
		g_flips_written.fetch_add(1, std::memory_order_relaxed);
	}
}

void GuestSubmitted() {
	if (!g_active) {
		return;
	}
	const uint32_t flips = g_flips_written.exchange(0, std::memory_order_relaxed);
	if (flips == 0) {
		return;
	}
	std::lock_guard guest(g_guest_mutex);
	g_guest_frame += flips;
	uint64_t value = 0;
	{
		std::lock_guard lock(g_mutex);
		if (g_swapchain == nullptr) {
			return;
		}
		if (const auto interval = Embed::GetOptions().low_latency_interval_us.load(std::memory_order_relaxed);
		    interval != g_interval_us) {
			SetSleepMode(interval);
		}
		Marker(g_guest_frame, VK_LATENCY_MARKER_SIMULATION_END_NV);
		VkLatencySleepInfoNV sleep {VK_STRUCTURE_TYPE_LATENCY_SLEEP_INFO_NV};
		sleep.signalSemaphore = g_semaphore;
		sleep.value           = g_semaphore_value + 1;
		if (Vk().vkLatencySleepNV(g_device, g_swapchain, &sleep) == VK_SUCCESS) {
			value = ++g_semaphore_value;
		}
	}
	if (value != 0) {
		// Outside the lock: this is where the driver delays the start of the next frame.
		VkSemaphoreWaitInfo wait {VK_STRUCTURE_TYPE_SEMAPHORE_WAIT_INFO};
		wait.semaphoreCount = 1;
		wait.pSemaphores    = &g_semaphore;
		wait.pValues        = &value;
		const bool timed_out = Vk().vkWaitSemaphores(g_device, &wait, SLEEP_LIMIT_NS) != VK_SUCCESS;
		std::lock_guard lock(g_mutex);
		g_sleeps++;
		g_timeouts += timed_out ? 1 : 0;
	}
	std::lock_guard lock(g_mutex);
	Marker(g_guest_frame + 1, VK_LATENCY_MARKER_SIMULATION_START_NV);
	Report();
}

uint64_t RenderSubmitted() {
	if (!g_active) {
		return 0;
	}
	std::lock_guard lock(g_mutex);
	const uint64_t  frame = ++g_render_frame;
	Marker(frame, VK_LATENCY_MARKER_RENDERSUBMIT_END_NV);
	Marker(frame + 1, VK_LATENCY_MARKER_RENDERSUBMIT_START_NV);
	return frame;
}

void PresentFrame(uint64_t frame) {
	if (frame != 0) {
		g_present_frame.store(frame, std::memory_order_relaxed);
	}
}

} // namespace Libs::Graphics::LowLatency
