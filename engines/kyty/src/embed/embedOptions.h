#ifndef KYTY_EMBED_EMBEDOPTIONS_H_
#define KYTY_EMBED_EMBEDOPTIONS_H_

// Options of the embedded renderer (embed/kytyEmbed.cpp fills them from KytyEmbedConfig before any
// subsystem starts). Header-only and all-default in the stand-alone emulator, so the hooks in
// upstream files that read them are no-ops there.

#include <array>
#include <atomic>
#include <cstdint>
#include <string>

namespace Embed {

struct Options {
	std::string window_title; // empty: the diagnostic title
	std::string ngx_data_dir;
	std::string fsr_dir;
	uint64_t    upscale_pass_hash   = 0;
	// Where the upscale pass keeps its inputs (graphics/host_gpu/renderer/upscalePass.h).
	std::array<uint8_t, 16> upscale_pass_layout {};
	uint32_t    rumble_scale        = 100;
	uint32_t    rumble_switch_floor = 0;
	uint8_t     pad_prefer          = 0;
	bool        wayland_native      = false;
	bool        low_latency         = false;
	bool        dlss                = false;
	bool        dlss_reverse_depth  = false;
	bool        ds_rumble           = true;
	bool        fsr_fg              = false;
	// Live: 1 000 000 / effective frame limit, 0 for none.
	std::atomic<uint32_t> low_latency_interval_us {0};
	// Live: (width << 32) | height the window should take, 0 for none.
	std::atomic<uint64_t> pending_window_size {0};
	// The host's frame limiter (KytyEmbedHooks).
	int64_t (*present_gate)(int64_t now, int64_t ticks_per_s, int flip_ready) = nullptr;
	void    (*presented)(int64_t now, int64_t ticks_per_s)                    = nullptr;
};

inline Options& GetOptions() {
	static Options options;
	return options;
}

constexpr int64_t TICKS_PER_S = 1000000000; // Common::SteadyNs

inline bool HasPresentGate() {
	return GetOptions().present_gate != nullptr;
}

// 0: present now. Otherwise the tick before which no flip may be presented.
inline uint64_t PresentGate(uint64_t now, bool flip_ready) {
	const auto gate  = GetOptions().present_gate;
	const auto until = gate != nullptr ? gate(static_cast<int64_t>(now), TICKS_PER_S, flip_ready ? 1 : 0) : 0;
	return until > 0 ? static_cast<uint64_t>(until) : 0;
}

inline void Presented(uint64_t now) {
	if (const auto presented = GetOptions().presented; presented != nullptr) {
		presented(static_cast<int64_t>(now), TICKS_PER_S);
	}
}

// The frame limit the session socket reports: the host's effective limit, 60 without one.
inline uint32_t FrameLimit() {
	const auto us = GetOptions().low_latency_interval_us.load(std::memory_order_relaxed);
	return us != 0 ? (1000000 + us / 2) / us : 60;
}

} // namespace Embed

#endif /* KYTY_EMBED_EMBEDOPTIONS_H_ */
