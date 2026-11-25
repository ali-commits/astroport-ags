#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGEN_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGEN_H_

// FSR 3.1 frame generation in the presenter: between two frames of the game the presenter shows
// one that AMD's frame interpolation (FidelityFX SDK 1.1.4, built from source as kyty_ffx) makes
// from them. On when the embedding host asks for it (Embed::Options::fsr_fg) in a build with
// KYTY_FSR_FRAMEGEN; without the build option every function here is an inline no-op.
//
// AMD's swapchain proxy is not used. The effect works on the presenter's own frames:
//  - Capture (render thread, at the title's upscale pass, upscalePass.cpp) copies the pass's depth
//    and motion vectors into a ring of kSlots image pairs and numbers the capture;
//  - a frame prepared for video out remembers the last capture (swapchain.cpp);
//  - Generator (present thread) records AMD's passes for a new main frame into the presenter's
//    command buffer and writes the frame between it and the one before into an image the
//    presenter then shows first; half a frame interval later it shows the real frame.
// Whatever does not fit shows the real frame alone, as without the feature.

#include "common/common.h"
#include "embed/embedOptions.h"
#include "graphics/host_gpu/vulkanCommon.h"

#include <cstdint>
#include <cstdio>
#include <memory>

struct SDL_Window;

namespace Libs::Graphics {
class CommandScheduler;
struct GraphicContext;
} // namespace Libs::Graphics

namespace Libs::Graphics::FrameGen {

// What the title's upscale pass read this frame. The images are in eTransferSrcOptimal.
struct CaptureInput {
	vk::Image    depth;
	vk::Format   depth_format;
	uint32_t     depth_layer;
	vk::Image    motion;
	vk::Format   motion_format;
	uint32_t     motion_level;
	uint32_t     motion_layer;
	vk::Extent2D render; // the part of both images the pass read, from their origin
	vk::Extent2D output; // what the pass upscales to
	float        jitter[2]; // in render pixels
	uint32_t     frame;     // the title's frame counter
};

// A frame of the presenter.
struct Source {
	vk::Image    image;
	vk::Extent2D extent;
	vk::Format   format;
	bool         hdr;     // HDR10 (PQ) values
	bool         blank;
	uint64_t     capture; // LastCapture() when the frame was prepared
};

#if KYTY_FSR_FRAMEGEN

// The host asked for it. Constant after start-up.
[[nodiscard]] inline bool Requested() {
	return Embed::GetOptions().fsr_fg;
}

// Whether the device can run AMD's shaders. False: the generator stays off for the run.
[[nodiscard]] bool DeviceSupports(vk::PhysicalDevice physical_device);
// Main thread, once the window exists: the refresh rate of its display.
void NoteDisplay(SDL_Window* window);
// After the presenter and the render context are gone: frees the capture ring's images.
void Shutdown();

// Whether the generator can use captures now: the upscale pass leaves its images alone otherwise.
[[nodiscard]] bool Capturing();
// Render thread, renderer's lock held: copies the inputs and numbers the capture. guest: the
// scheduler `command` belongs to.
void Capture(CommandScheduler& guest, vk::CommandBuffer command, const CaptureInput& input);
// The number of the last capture recorded, 0 before the first.
[[nodiscard]] uint64_t LastCapture();

// Present thread only.
class Generator final {
public:
	// scheduler: the presenter's, on whose command buffers Record is called.
	Generator(GraphicContext& graphics, CommandScheduler& scheduler);
	~Generator();
	KYTY_CLASS_NO_COPY(Generator);

	// A new main frame is about to be shown. False: show it alone. True: AMD's contexts fit it;
	// record with Record into an image of the frame's size and format that has storage usage.
	[[nodiscard]] bool Prepare(const Source& frame, vk::PresentModeKHR mode);
	// frame is in eShaderReadOnlyOptimal, output in eGeneral; both are left so. True: output holds
	// the frame to show before `frame`. False: output is not to be shown (the commands recorded,
	// if any, are still submitted).
	[[nodiscard]] bool Record(vk::CommandBuffer command, const Source& frame, vk::Image output);
	// After the generated frame was presented at shown_ns: waits until the real one is due.
	void Pace(vk::PresentModeKHR mode, uint64_t shown_ns);

private:
	struct Impl;
	std::unique_ptr<Impl> m_impl;
};

#else

// A build without the SDK says so once when the host asks for the feature.
[[nodiscard]] inline bool Requested() {
	if (Embed::GetOptions().fsr_fg) {
		static const bool said = [] {
			std::printf("FSR frame generation: not built in\n");
			std::fflush(stdout);
			return true;
		}();
		(void)said;
	}
	return false;
}

[[nodiscard]] inline bool     DeviceSupports(vk::PhysicalDevice /*physical_device*/) {
	return false;
}
inline void                   NoteDisplay(SDL_Window* /*window*/) {}
inline void                   Shutdown() {}
[[nodiscard]] inline bool     Capturing() {
	return false;
}
inline void                   Capture(CommandScheduler& /*guest*/, vk::CommandBuffer /*command*/,
                                      const CaptureInput& /*input*/) {}
[[nodiscard]] inline uint64_t LastCapture() {
	return 0;
}

class Generator final {
public:
	Generator(GraphicContext& /*graphics*/, CommandScheduler& /*scheduler*/) {}
	[[nodiscard]] bool Prepare(const Source& /*frame*/, vk::PresentModeKHR /*mode*/) { return false; }
	[[nodiscard]] bool Record(vk::CommandBuffer /*command*/, const Source& /*frame*/, vk::Image /*output*/) {
		return false;
	}
	void Pace(vk::PresentModeKHR /*mode*/, uint64_t /*shown_ns*/) {}
};

#endif // KYTY_FSR_FRAMEGEN

} // namespace Libs::Graphics::FrameGen

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGEN_H_
