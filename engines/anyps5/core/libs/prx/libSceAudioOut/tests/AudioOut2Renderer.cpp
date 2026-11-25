#include "prx/libSceAudioOut/include/AudioOut.hpp"
#include "prx/libSceAudioOut/include/AudioOut2.hpp"
#include "prx/libScePad/include/PadState.hpp"
#include "prx/libkernel/include/ExternalRenderer.hpp"
#include "SceTypes.hpp"

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <mutex>
#include <random>
#include <string>
#include <thread>
#include <vector>

namespace {

constexpr std::uint32_t grain = 256;
constexpr std::uint32_t frequency = 48000;
constexpr std::uint16_t portTypePadSpeaker = 0x3;
constexpr std::uint16_t portTypePadVibration = 0x6;
constexpr std::uint32_t formatFloat = 0;

void Require(bool value, const char* what) {
    if (value) return;
    std::fprintf(stderr, "audio out2 renderer check failed: %s\n", what);
    std::abort();
}

std::mutex g_lock;
std::atomic<bool> g_accepts{false};
std::atomic<float> g_gain{2.0f};
int g_grains[2] = {};
float g_peak[2] = {};
bool g_wellFormed = true;

std::uint64_t PadAudio(std::uint32_t rate, int speaker, const void* samples, std::uint32_t frames, std::uint32_t channels, int isFloat, float gain) {
    std::lock_guard lock(g_lock);
    g_wellFormed = g_wellFormed && rate == frequency && frames == grain && channels == 2 && isFloat == 1 && gain == 1.0f && (speaker == 0 || speaker == 1);
    g_grains[speaker]++;
    for (std::uint32_t index = 0; index < frames * channels; ++index) g_peak[speaker] = std::max(g_peak[speaker], std::abs(static_cast<const float*>(samples)[index]));
    return g_accepts.load() ? 5333 : 0;
}

float HapticGain() {
    return g_gain.load();
}

void ResetSeen() {
    std::lock_guard lock(g_lock);
    g_grains[0] = g_grains[1] = 0;
    g_peak[0] = g_peak[1] = 0.0f;
}

AudioOut2PortHandle CreatePort(AudioOut2ContextHandle context, std::uint16_t type, std::uint32_t channels) {
    AudioOut2PortParam params{};
    params.port_type = type;
    params.data_format = channels << 8 | formatFloat;
    params.sampling_freq = frequency;
    AudioOut2PortHandle port = 0;
    Require(sceAudioOut2PortCreate(context, &params, &port) == 0, "a pad port is created");
    const std::vector<float> volume(channels, 1.0f);
    const AudioOut2Attribute attribute{1, 0, volume.data(), volume.size() * sizeof(float)};
    Require(sceAudioOut2PortSetAttributes(port, &attribute, 1) == 0, "port volume is set");
    return port;
}

void SetData(AudioOut2PortHandle port, const void* data) {
    const AudioOut2Attribute attribute{0, 0, &data, sizeof(data)};
    Require(sceAudioOut2PortSetAttributes(port, &attribute, 1) == 0, "port data is set");
}

void SetEnvironment(const char* name, const std::string& value) {
#ifdef _WIN32
    _putenv_s(name, value.c_str());
#else
    setenv(name, value.c_str(), 1);
#endif
}

}

int main() {
    const auto path = std::filesystem::temp_directory_path() / ("anyps5_audio_out2_renderer-" + std::to_string(std::random_device{}()) + ".raw");
    SetEnvironment("SDL_AUDIODRIVER", "disk");
    SetEnvironment("SDL_DISKAUDIOFILE", path.string());

    static ExternalRenderer renderer{};
    renderer.size = sizeof(renderer);
    renderer.padAudio = PadAudio;
    renderer.hapticGain = HapticGain;
    ExternalRendererSet_nid_no_patch(&renderer);

    AudioOut2ContextParam params{};
    Require(sceAudioOut2ContextResetParam(&params) == 0, "context parameters reset");
    params.num_grains = grain;
    params.queue_depth = 1;
    AudioOut2ContextHandle context = 0;
    Require(sceAudioOut2ContextCreate(&params, nullptr, 0, &context) == 0 && context != 0, "a context is created");
    const auto vibrationPort = CreatePort(context, portTypePadVibration, 2);
    const auto speakerPort = CreatePort(context, portTypePadSpeaker, 1);
    const std::vector<float> vibration(static_cast<std::size_t>(grain) * 2, 0.25f);
    const std::vector<float> speaker(grain, 0.125f);
    const auto push = [&](int grains) {
        for (int index = 0; index < grains; ++index) {
            SetData(vibrationPort, vibration.data());
            SetData(speakerPort, speaker.data());
            Require(sceAudioOut2ContextPush(context, 1) == 0, "a grain is pushed");
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
    };
    std::uint32_t sequence = 0;
    PadOutputState output;

    push(24);
    {
        std::lock_guard lock(g_lock);
        Require(g_wellFormed && g_grains[0] > 0 && g_grains[1] > 0, "a refusing renderer is still offered both kinds of grain");
        Require(g_peak[0] == 0.5f, "haptic grains carry the live gain");
        Require(g_peak[1] == 0.0f, "a refused speaker stays in the main mix");
    }
    Require(PadFetchOutput_nid_postfix(&sequence, &output) && output.audioVibrationLarge != 0, "refused haptics fall back to derived rumble");

    g_accepts.store(true);
    g_gain.store(9.0f);
    push(24);
    ResetSeen();
    std::this_thread::sleep_for(std::chrono::milliseconds(120));
    push(24);
    {
        std::lock_guard lock(g_lock);
        Require(g_wellFormed && g_grains[0] > 0 && g_peak[0] == 1.0f, "the gain is limited to 4 and the samples to full scale");
        Require(g_peak[1] == 0.125f, "an accepted speaker plays through the renderer");
    }
    Require(PadFetchOutput_nid_postfix(&sequence, &output), "the derived rumble ends");
    Require(output.audioVibrationLarge == 0 && output.audioVibrationSmall == 0, "accepted haptics leave no derived rumble");

    Require(sceAudioOut2PortDestroy(vibrationPort) == 0 && sceAudioOut2PortDestroy(speakerPort) == 0 && sceAudioOut2ContextDestroy(context) == 0, "the context is destroyed");

    std::this_thread::sleep_for(std::chrono::milliseconds(120));
    ResetSeen();
    g_gain.store(2.0f);
    const int classic = sceAudioOutOpen(0x10000000, 10, 0, grain, frequency, 1);
    Require(classic > 0, "a classic vibration port is opened");
    int volume[2] = {16384, 16384};
    Require(sceAudioOutSetVolume(classic, 3, volume) == 0, "the classic port volume is set");
    const std::vector<std::int16_t> pcm(static_cast<std::size_t>(grain) * 2, 2048);
    Require(sceAudioOutOutput(classic, pcm.data()) == static_cast<int>(grain), "a classic grain is output");
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
    for (;;) {
        {
            std::lock_guard lock(g_lock);
            if (g_grains[0] > 0) break;
        }
        Require(std::chrono::steady_clock::now() < deadline, "the classic grain reaches the renderer");
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
    {
        std::lock_guard lock(g_lock);
        Require(g_wellFormed && g_peak[0] == 0.0625f && g_grains[1] == 0, "a classic vibration grain is converted, scaled by volume and gain and sent as haptics");
    }
    Require(!PadFetchOutput_nid_postfix(&sequence, &output) || (output.audioVibrationLarge == 0 && output.audioVibrationSmall == 0), "an accepted classic grain leaves no derived rumble");
    g_accepts.store(false);
    while (!PadFetchOutput_nid_postfix(&sequence, &output) || output.audioVibrationLarge == 0) {
        Require(std::chrono::steady_clock::now() < deadline, "a refused classic grain falls back to derived rumble");
        Require(sceAudioOutOutput(classic, pcm.data()) == static_cast<int>(grain), "a classic grain is output");
    }
    Require(sceAudioOutClose(classic) == 0, "the classic port is closed");

    std::error_code error;
    std::filesystem::remove(path, error);
    std::puts("audio out2 renderer checks passed");
    return 0;
}
