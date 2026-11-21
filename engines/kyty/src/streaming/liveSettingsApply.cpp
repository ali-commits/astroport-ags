#include "streaming/liveSettings.h"

#include "graphics/presentation/videoOut.h"
#include "graphics/presentation/window/windowInternal.h"

#include <cstdlib>

#if defined(__linux__)
#include <dlfcn.h>
#endif

namespace Streaming {

int LivePatch(const char* name, bool on) {
#if defined(__linux__)
	using Patch = int32_t (*)(const char*, int32_t);
	static const auto patch = []() -> Patch {
		const char* audit = std::getenv("LD_AUDIT");
		if (audit == nullptr) return nullptr;
		std::string_view entries(audit);
		while (!entries.empty()) {
			const auto separator = entries.find_first_of(": ");
			const auto path = entries.substr(0, separator);
			if (path.ends_with("libastro_patcher.so")) {
				void* library = dlopen(std::string(path).c_str(), RTLD_NOW | RTLD_LOCAL);
				if (library == nullptr) return nullptr;
				auto symbol = reinterpret_cast<Patch>(dlsym(library, "astro_patch_live"));
				if (symbol == nullptr) dlclose(library);
				return symbol;
			}
			entries.remove_prefix(separator == std::string_view::npos ? entries.size() : separator + 1);
		}
		return nullptr;
	}();
	if (patch != nullptr) return patch(name, on ? 1 : 0);
#else
	(void)name;
	(void)on;
#endif
	return -10;
}

Json ApplySettings(const Json& apply) {
	const auto now = Live().load();
	auto decision = DecideSettingsApply(apply, now);
	auto& actions = decision.actions;

	if (decision.cap_swap) {
		const auto [old_fps, new_fps] = *decision.cap_swap;
		const auto old_cap = "precise frame cap " + std::to_string(old_fps) + " fps";
		const auto new_cap = "precise frame cap " + std::to_string(new_fps) + " fps";
		// shortcut: ASTROHYBRID_LOW_LATENCY_INTERVAL_US keeps its launch value; update it when a group gets a second value
		if (LivePatch(old_cap.c_str(), false) != 0 || LivePatch(new_cap.c_str(), true) != 0) {
			LivePatch(old_cap.c_str(), true);
			decision.reply["applied"].erase("display.fps");
			decision.reply["refused"]["display.fps"] = "Applies at the next start";
			actions.fps.reset();
		}
	}

	if (actions.fps) Libs::VideoOut::SetFrameLimit(*actions.fps);
	if (actions.size && !now.exporting) {
		const auto [width, height] = *actions.size;
		Libs::Graphics::WindowRequestSize(width, height);
	}
	Live().Update([&](LiveState& state) {
		if (actions.fps) state.fps = *actions.fps;
		if (actions.size) {
			state.width = actions.size->first;
			state.height = actions.size->second;
		}
		if (actions.haptic) state.haptic = *actions.haptic;
	});
	return std::move(decision.reply);
}

} // namespace Streaming
