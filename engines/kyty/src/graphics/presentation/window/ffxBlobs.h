#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXBLOBS_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXBLOBS_H_

// The shaders of AMD's frame interpolation and optical flow, built from the SDK's GLSL by Kyty's
// own build (CMakeLists.txt, KYTY_FSR_FRAMEGEN). ffxBlobs.cpp also defines the two functions the
// SDK's Vulkan backend calls to get a shader with its binding lists, ffxGetPermutationBlobByIndex
// and ffxIsWave64, in place of AMD's generated accessors.

#include <cstdint>
#include <span>

namespace Libs::Graphics::FfxBlobs {

struct Row {
	uint32_t                  effect; // FfxEffect
	uint32_t                  pass;   // FfxFrameInterpolationPass or FfxOpticalflowPass
	uint32_t                  key;    // frame interpolation: depth is reversed; optical flow: HDR input
	std::span<const uint32_t> words;  // SPIR-V
};

// Every module: 11 frame-interpolation and 7 optical-flow passes, each for key 0 and 1.
[[nodiscard]] std::span<const Row> Rows();

} // namespace Libs::Graphics::FfxBlobs

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXBLOBS_H_
