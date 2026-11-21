#pragma once

#include "streaming/protocol.h"

#include <span>
#include <utility>

namespace Streaming {

struct LiveState {
	uint32_t fps;
	uint32_t width, height;
	float haptic;
	bool fullscreen;
	bool exporting;
};

struct FrameCapRule {
	uint32_t fps;
	uint8_t group;
};

inline constexpr std::array<FrameCapRule, 2> kFrameCapGroups {{{30, 0}, {60, 1}}};
inline constexpr std::string_view kHapticKey = "controller.haptics";

struct LiveActions {
	std::optional<uint32_t> fps;
	std::optional<std::pair<uint32_t, uint32_t>> size;
	std::optional<float> haptic;
};

struct Decision {
	Json reply;
	LiveActions actions;
	std::optional<std::pair<uint32_t, uint32_t>> cap_swap; // old, new
};

Decision DecideSettingsApply(const Json& apply, const LiveState& now,
                             std::span<const FrameCapRule> groups = kFrameCapGroups);

// LiveState is too large for a lock-free atomic. Keep the whole snapshot under one lock.
class LiveStateStorage {
public:
	explicit LiveStateStorage(LiveState state): m_state(state) {}

	LiveState load() const {
		std::lock_guard lock(m_mutex);
		return m_state;
	}

	void store(LiveState state) {
		Update([&](LiveState& current) { current = state; });
	}

	template <typename UpdateState>
	void Update(UpdateState update) {
		std::lock_guard lock(m_mutex);
		update(m_state);
	}

private:
	mutable std::mutex m_mutex;
	LiveState m_state;
};

LiveStateStorage& Live();
float PadVibrationGain();
int LivePatch(const char* name, bool on);
Json ApplySettings(const Json& apply);

} // namespace Streaming
