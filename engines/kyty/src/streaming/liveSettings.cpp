#include "streaming/liveSettings.h"
#include "streaming/transport.h"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdlib>

namespace Streaming {
namespace {

std::optional<std::pair<uint32_t, uint32_t>> ParseResolution(const Json& value) {
	if (!value.is_string()) return std::nullopt;
	const auto& text = value.get_ref<const std::string&>();
	const auto separator = text.find('x');
	if (separator == std::string::npos) return std::nullopt;

	const auto parse_side = [](std::string_view side, uint32_t& size) {
		const auto* end = side.data() + side.size();
		const auto result = std::from_chars(side.data(), end, size);
		return result.ec == std::errc{} && result.ptr == end && size >= 2 && size <= 8192;
	};
	uint32_t width = 0, height = 0;
	const std::string_view resolution = text;
	if (!parse_side(resolution.substr(0, separator), width) ||
	    !parse_side(resolution.substr(separator + 1), height)) return std::nullopt;
	return std::pair{width, height};
}

uint32_t ReadLaunchInteger(const char* key, uint32_t fallback, uint32_t min, uint32_t max) {
	const char* value = std::getenv(key);
	if (value == nullptr) return fallback;
	char* end = nullptr;
	const auto parsed = std::strtoul(value, &end, 10);
	return end != value && *end == '\0' && parsed >= min && parsed <= max
	           ? static_cast<uint32_t>(parsed) : fallback;
}

float LaunchHapticGain() {
	const char* value = std::getenv("APS5_PAD_VIBRATION_GAIN");
	if (value == nullptr) return 1.0f;
	char* end = nullptr;
	const float gain = std::strtof(value, &end);
	return end != value && *end == '\0' && !std::isnan(gain) ? std::clamp(gain, 0.0f, 4.0f) : 1.0f;
}

} // namespace

Decision DecideSettingsApply(const Json& apply, const LiveState& now,
                             std::span<const FrameCapRule> groups) {
	Decision decision {{ {"type", "settings.applied"}, {"id", apply.value("id", Json{})},
	                     {"applied", Json::object()}, {"values", Json::object()},
	                     {"refused", Json::object()} }, {}, {}};
	const auto requested = apply.find("values");
	if (requested == apply.end() || !requested->is_object()) return decision;

	auto& applied = decision.reply["applied"];
	auto& refused = decision.reply["refused"];
	for (const auto& [key, value]: requested->items()) {
		if (key == "display.fps" && value.is_number_integer()) {
			const auto fps = value.get<uint64_t>();
			const auto rule = std::ranges::find(groups, fps, &FrameCapRule::fps);
			if (rule != groups.end()) {
				if (rule->fps != now.fps) {
					const auto current = std::ranges::find(groups, now.fps, &FrameCapRule::fps);
					if (current == groups.end() || current->group != rule->group) {
						refused[key] = "Applies at the next start";
						continue;
					}
					decision.cap_swap = std::pair{now.fps, rule->fps};
				}
				decision.actions.fps = rule->fps;
				applied[key] = value;
				continue;
			}
		} else if (key == "display.resolution") {
			if (const auto size = ParseResolution(value)) {
				if (now.fullscreen && !now.exporting) {
					refused[key] = "Leave fullscreen first";
				} else {
					decision.actions.size = size;
					applied[key] = value;
				}
				continue;
			}
		} else if (key == kHapticKey && value.is_number_integer()) {
			const auto percent = value.get<uint64_t>();
			if (percent >= 100 && percent <= 400) {
				decision.actions.haptic = static_cast<float>(percent) / 100.0f;
				decision.reply["values"][key] = value;
				continue;
			}
		}
		refused[key] = "Not supported";
	}
	return decision;
}

LiveStateStorage& Live() {
	static LiveStateStorage state({
	    ReadLaunchInteger("ASTROHYBRID_FPS_LIMIT", 60, 10, 240),
	    ReadLaunchInteger("KYTY_GPU_WIDTH", 1280, 2, 8192),
	    ReadLaunchInteger("KYTY_GPU_HEIGHT", 720, 2, 8192),
	    LaunchHapticGain(), false, false,
	});
	return state;
}

float PadVibrationGain() {
	return Live().load().haptic;
}

OutputState ReadLiveOutput() {
	const auto state = Live().load();
	return {state.width, state.height, state.fps};
}

} // namespace Streaming
