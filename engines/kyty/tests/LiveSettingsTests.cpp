#include "streaming/liveSettings.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>

using namespace Streaming;

static void Check(bool condition, const char* message) {
	if (!condition) {
		std::fprintf(stderr, "%s\n", message);
		std::exit(1);
	}
}

static const LiveState kNow {30, 3840, 2160, 2.5f, false, false};
static Json Apply(Json values) { return {{"type", "settings.apply"}, {"id", 7}, {"values", values}}; }
static void TestFrameRate() {
  auto d = DecideSettingsApply(Apply({{"display.fps", 60}}), kNow);
  Check(d.reply["refused"]["display.fps"] == "Applies at the next start" && !d.actions.fps && !d.cap_swap, "30 to 60 refused");
  d = DecideSettingsApply(Apply({{"display.fps", 30}}), kNow);
  Check(d.reply["applied"]["display.fps"] == 30 && !d.cap_swap, "same value applied");
  const std::array<FrameCapRule, 2> one_group {{{30, 0}, {60, 0}}};
  d = DecideSettingsApply(Apply({{"display.fps", 60}}), kNow, one_group);
  Check(d.actions.fps == 60u && d.cap_swap == std::pair<uint32_t, uint32_t>{30, 60}, "same group swaps the cap");
  for (Json bad : {Json(45), Json("60"), Json(60.5)})
    Check(DecideSettingsApply(Apply({{"display.fps", bad}}), kNow).reply["refused"]["display.fps"] == "Not supported", "bad fps");
}
static void TestResolution() {
  auto d = DecideSettingsApply(Apply({{"display.resolution", "2560x1440"}}), kNow);
  Check(d.actions.size == std::pair<uint32_t, uint32_t>{2560, 1440} && d.reply["applied"]["display.resolution"] == "2560x1440", "windowed resize");
  for (const char* bad : {"abcx", "0x0", "9000x9000", "1920x", "1920x1080x2"})
    Check(DecideSettingsApply(Apply({{"display.resolution", bad}}), kNow).reply["refused"]["display.resolution"] == "Not supported", bad);
  LiveState full = kNow; full.fullscreen = true;
  Check(DecideSettingsApply(Apply({{"display.resolution", "1920x1080"}}), full).reply["refused"]["display.resolution"] == "Leave fullscreen first", "fullscreen local");
  full.exporting = true;
  Check(DecideSettingsApply(Apply({{"display.resolution", "1920x1080"}}), full).actions.size.has_value(), "fullscreen export follows");
}
static void TestHapticAndMixed() {
  auto d = DecideSettingsApply(Apply({{"controller.haptics", 300}, {"display.fps", "x"}, {"nope", 1}}), kNow);
  Check(d.actions.haptic == 3.0f && d.reply["values"]["controller.haptics"] == 300 && d.reply["applied"].empty()
        && d.reply["refused"].size() == 2 && d.reply["id"] == 7 && d.reply["type"] == "settings.applied", "mixed packet");
  for (Json bad : {Json(50), Json(450), Json("2"), Json(std::nan(""))})
    Check(DecideSettingsApply(Apply({{"controller.haptics", bad}}), kNow).reply["refused"].contains("controller.haptics"), "bad haptic");
  d = DecideSettingsApply(Json{{"type", "settings.apply"}, {"id", 8}}, kNow);
  Check(d.reply["id"] == 8 && d.reply["applied"].empty() && d.reply["values"].empty() && d.reply["refused"].empty(), "no values");
}
int main() { TestFrameRate(); TestResolution(); TestHapticAndMixed(); std::puts("live settings tests: ok"); }
