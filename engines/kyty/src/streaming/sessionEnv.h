#pragma once

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace Streaming {

struct SessionSocketChoice {
	const char* path = nullptr;
};

[[nodiscard]] inline SessionSocketChoice SelectSessionSocket(const char* ags_socket,
                                                             const char* ags_protocol) noexcept {
	if (ags_socket == nullptr || *ags_socket == '\0') return {};
	if (ags_protocol != nullptr && std::strcmp(ags_protocol, "2") == 0) return {ags_socket};

	static std::atomic_flag logged = ATOMIC_FLAG_INIT;
	if (!logged.test_and_set(std::memory_order_relaxed))
		std::fprintf(stderr, "AGS session socket ignored: AGS_SESSION_PROTOCOL must be 2\n");
	return {};
}

[[nodiscard]] inline const char* SessionSocket() noexcept {
	return SelectSessionSocket(std::getenv("AGS_SESSION_SOCKET"),
	                           std::getenv("AGS_SESSION_PROTOCOL")).path;
}

[[nodiscard]] inline bool FramesRequested(const char* kyty_stream_frames) noexcept {
	return kyty_stream_frames == nullptr || std::strcmp(kyty_stream_frames, "0") != 0;
}

} // namespace Streaming
