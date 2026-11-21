#include "streaming/metricsSource.h"

#include <memory>
#include <utility>

namespace Libs::Graphics {
uint64_t PresentedFrames();
}

namespace Streaming {
namespace {
struct Sample {
	std::chrono::steady_clock::time_point time;
	uint64_t frames;
};
} // namespace

MetricsHooks MakeEmulatorMetricsHooks() {
	const auto previous = std::make_shared<std::optional<Sample>>();
	return {
	    [previous](MetricMask keys) {
		    if (keys == 0) previous->reset();
	    },
	    [previous](MetricMask) {
		    const Sample current {std::chrono::steady_clock::now(), Libs::Graphics::PresentedFrames()};
		    const auto last = std::exchange(*previous, current);
		    MetricValues values;
		    if (last && current.time > last->time && current.frames >= last->frames) {
			    const auto seconds = std::chrono::duration<double>(current.time - last->time).count();
			    const double fps = static_cast<double>(current.frames - last->frames) / seconds;
			    values.Set(Metric::GameFps, fps);
			    if (fps > 0) values.Set(Metric::FrameTimeMs, 1000.0 / fps);
		    }
		    return values;
	    },
	};
}

} // namespace Streaming
