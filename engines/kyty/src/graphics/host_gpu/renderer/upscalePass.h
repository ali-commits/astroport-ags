#ifndef EMULATOR_SRC_GRAPHICS_HOST_GPU_RENDERER_UPSCALEPASS_H_
#define EMULATOR_SRC_GRAPHICS_HOST_GPU_RENDERER_UPSCALEPASS_H_

#include "embed/embedOptions.h"
#include "graphics/guest_gpu/pssr.h"

#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <optional>
#include <span>
#include <string>

// The embedded renderer replaces one pass of the title, its own temporal upscale, by DLSS (dlss.h)
// writing the same output target. Which pass that is and where it keeps its inputs comes from the
// host (Embed::Options): nothing here names a title. Whatever does not fit leaves the title's pass
// to run. The draw-side code is RenderExecutor::UpscalePassDraw (upscalePass.cpp).

namespace Libs::Graphics {
struct RenderColorInfo;
struct RenderDepthInfo;
} // namespace Libs::Graphics

namespace Libs::Graphics::UpscalePass {

// Embed::Options::upscale_pass_layout.
struct Layout {
	uint8_t jitter[2];    // constant words: this frame's jitter in UV
	uint8_t output[2];    // output width and height
	uint8_t input[2];     // input width and height
	uint8_t frame;        // frame counter
	uint8_t motion_used;  // nonzero while the motion vectors are in use
	uint8_t motion_image; // among the pixel shader's images
	uint8_t color_image;
	uint8_t constants;    // among the pixel shader's buffers
	uint8_t given;        // 0: the host gave no layout
	uint8_t reserved[4];
};
static_assert(sizeof(Layout) == sizeof(Embed::Options::upscale_pass_layout));

[[nodiscard]] inline Layout GetLayout() {
	Layout layout {};
	std::memcpy(&layout, Embed::GetOptions().upscale_pass_layout.data(), sizeof(layout));
	return layout;
}

// Whether draws are looked at. One load per draw in the stand-alone emulator, where it is false.
[[nodiscard]] inline bool Enabled() {
	const auto& options = Embed::GetOptions();
	return options.dlss && options.upscale_pass_hash != 0;
}

// The constant words the layout reads.
[[nodiscard]] inline uint32_t WordCount(const Layout& layout) {
	return 1u + std::max({layout.jitter[0], layout.jitter[1], layout.output[0], layout.output[1],
	                      layout.input[0], layout.input[1], layout.frame, layout.motion_used});
}

// A size or a counter in a constant word; shaders keep them as integers or as floats.
[[nodiscard]] inline uint32_t Whole(uint32_t word) {
	if (word < 0x3f800000u || word >= 0x7f800000u) {
		return word;
	}
	const auto value = std::bit_cast<float>(word);
	return value < 4294967040.0f ? static_cast<uint32_t>(value) : UINT32_MAX;
}

struct Constants {
	float    jitter[2];
	uint32_t output[2];
	uint32_t input[2];
	uint32_t frame;
	bool     motion_used;

	// Sizes an upscale can have and a jitter within the pixel (UV times input size).
	[[nodiscard]] bool Valid() const {
		return input[0] != 0 && input[1] != 0 && input[0] <= output[0] && input[1] <= output[1] &&
		       output[0] <= 16384 && output[1] <= 16384 &&
		       std::abs(jitter[0] * static_cast<float>(input[0])) <= 1.0f &&
		       std::abs(jitter[1] * static_cast<float>(input[1])) <= 1.0f;
	}
};

// nullopt without a layout or with fewer words than it reads.
[[nodiscard]] inline std::optional<Constants> Read(const Layout&             layout,
                                                   std::span<const uint32_t> words) {
	if (layout.given == 0 || words.size() < WordCount(layout)) {
		return std::nullopt;
	}
	return Constants {
	    {std::bit_cast<float>(words[layout.jitter[0]]), std::bit_cast<float>(words[layout.jitter[1]])},
	    {Whole(words[layout.output[0]]), Whole(words[layout.output[1]])},
	    {Whole(words[layout.input[0]]), Whole(words[layout.input[1]])},
	    Whole(words[layout.frame]),
	    words[layout.motion_used] != 0};
}

// The pass's motion vectors are in UV, point from the previous to the current position and
// include the jitter. last_frame: the frame counter of the last frame DLSS upscaled.
[[nodiscard]] inline Pssr::Request MakeRequest(const Constants& constants, bool reverse_depth,
                                               std::optional<uint32_t> last_frame,
                                               uint64_t                context) {
	Pssr::Request request {};
	request.width           = constants.input[0];
	request.height          = constants.input[1];
	request.context         = context;
	request.jitter[0]       = constants.jitter[0] * static_cast<float>(constants.input[0]);
	request.jitter[1]       = constants.jitter[1] * static_cast<float>(constants.input[1]);
	request.motion_scale[0] = -static_cast<float>(constants.input[0]);
	request.motion_scale[1] = -static_cast<float>(constants.input[1]);
	request.pre_exposure    = 1.0f;
	request.flags           = Pssr::kAutoExposure | Pssr::kJitteredMotion;
	if (reverse_depth) {
		request.flags |= Pssr::kReverseDepth;
	}
	if (!last_frame || constants.frame != *last_frame + 1u) {
		request.flags |= Pssr::kReset;
	}
	return request;
}

// What the depth target of the most recent colour targets was. Depth is small and copied.
template <typename Depth, size_t Size = 16>
class DepthRing {
public:
	void Note(uint64_t color, const Depth& depth) {
		for (auto& entry: m_entries) {
			if (entry.color == color) {
				entry.depth = depth;
				return;
			}
		}
		m_entries[m_next] = {color, depth};
		m_next            = (m_next + 1) % Size;
	}
	[[nodiscard]] const Depth* Find(uint64_t color) const {
		for (const auto& entry: m_entries) {
			if (entry.color == color && color != 0) {
				return &entry.depth;
			}
		}
		return nullptr;
	}

private:
	struct Entry {
		uint64_t color = 0;
		Depth    depth {};
	};
	std::array<Entry, Size> m_entries {};
	size_t                  m_next = 0;
};

enum class Outcome : uint8_t {
	Evaluated,
	Off,       // NGX did not start
	Continued, // the draw continues the previous one
	Bindings,  // not the images, buffers or target the layout names
	Constants, // unreadable constants or no layout
	Sizes,
	NoMotion, // motion vectors unused this frame
	NoDepth,
	Images,  // a format, sample count or usage DLSS cannot take
	Refused, // no quality mode for the sizes, or creation or evaluation failed
	Count
};

struct Stats {
	std::array<uint32_t, static_cast<size_t>(Outcome::Count)> counts {};

	void Add(Outcome outcome) { counts[static_cast<size_t>(outcome)]++; }
	// The periodic line, empty when the pass was not drawn since the last one.
	[[nodiscard]] std::string Take() {
		static constexpr std::array<const char*, static_cast<size_t>(Outcome::Count)> NAMES {
		    "evaluated", "NGX off",     "continued draw", "bindings", "constants",
		    "sizes",     "motion unused", "no depth",     "images",   "refused"};
		std::string fallbacks;
		uint32_t    total = 0;
		for (size_t i = 0; i < counts.size(); i++) {
			total += counts[i];
			if (i != 0 && counts[i] != 0) {
				fallbacks += (fallbacks.empty() ? " " : ", ") + std::string(NAMES[i]) + " " +
				             std::to_string(counts[i]);
			}
		}
		if (total == 0) {
			return {};
		}
		auto text = "DLSS pass: " + std::to_string(counts[0]) + " evaluated, " +
		            (fallbacks.empty() ? std::string("no fallbacks") : "fallbacks:" + fallbacks);
		counts    = {};
		return text;
	}
};

// Every draw's targets, for the depth lookup.
void NoteTargets(std::span<const RenderColorInfo> colors, const RenderDepthInfo& depth);

} // namespace Libs::Graphics::UpscalePass

#endif // EMULATOR_SRC_GRAPHICS_HOST_GPU_RENDERER_UPSCALEPASS_H_
