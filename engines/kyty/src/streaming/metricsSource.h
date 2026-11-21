#pragma once

#include "streaming/metrics.h"

namespace Streaming {
// Samples the window's presented-frame counter at the subscribed interval.
MetricsHooks MakeEmulatorMetricsHooks();
} // namespace Streaming
