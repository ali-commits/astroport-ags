#include "prx/libSceAudioOut/src/AudioOut2PadMix.hpp"

#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <limits>
#include <mutex>
#include <stdexcept>
#include <thread>
#include <vector>

static void Require(bool value, const char*) {
    if (!value) std::abort();
}

static std::mutex g_sinkGate;
static std::mutex g_sinkLock;
static std::vector<float> g_sinkSeen;
static std::atomic<int> g_sinkCalls{0};
static std::atomic<bool> g_sinkAccepts{false};

static std::uint64_t Sink(std::uint32_t rate, int speaker, const void* samples, std::uint32_t frames, std::uint32_t channels, int isFloat, float gain) {
    Require(rate == 48000 && frames == 1 && channels == 2 && isFloat == 1 && gain == 1.0f, "the sink receives the grain as it was pushed");
    {
        std::lock_guard seen(g_sinkLock);
        g_sinkSeen.push_back(static_cast<const float*>(samples)[0] + (speaker != 0 ? 1000.0f : 0.0f));
    }
    g_sinkCalls.fetch_add(1);
    std::lock_guard gate(g_sinkGate);
    return g_sinkAccepts.load() ? 5000 : 0;
}

static void WaitForSinkCalls(int calls) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
    while (g_sinkCalls.load() < calls) {
        Require(std::chrono::steady_clock::now() < deadline, "the worker delivers queued grains");
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
}

static void CheckPadAudioQueue() {
    auto& queue = *new AudioOut2PadAudioQueue();
    const auto push = [&](bool speaker, float value) { return queue.Push(Sink, 48000, speaker, 2, {value, value}); };
    {
        std::unique_lock gate(g_sinkGate);
        Require(!push(false, 0.0f), "nothing is accepted before the sink answered");
        WaitForSinkCalls(1);
        for (int grain = 1; grain <= 40; ++grain) Require(!push(false, static_cast<float>(grain)), "a push never waits for the sink");
    }
    WaitForSinkCalls(1 + static_cast<int>(AudioOut2PadAudioQueue::MAX_GRAINS));
    {
        std::lock_guard seen(g_sinkLock);
        Require(g_sinkSeen.size() == 1 + AudioOut2PadAudioQueue::MAX_GRAINS && g_sinkSeen[0] == 0.0f, "the grain in flight is delivered");
        for (std::size_t index = 1; index < g_sinkSeen.size(); ++index) Require(g_sinkSeen[index] == static_cast<float>(8 + index), "the oldest waiting grains are dropped, the rest keep their order");
    }
    const auto eventually = [&](bool speaker, bool expected) {
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
        while (push(speaker, 50.0f) != expected) {
            Require(std::chrono::steady_clock::now() < deadline, "the sink's answer reaches the next pushes");
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
    };
    g_sinkAccepts.store(true);
    eventually(false, true);
    Require(!push(true, 52.0f), "speaker and haptic acceptance are separate");
    eventually(true, true);
    g_sinkAccepts.store(false);
    eventually(false, false);
    eventually(true, false);
    std::lock_guard seen(g_sinkLock);
    bool speakerSeen = false;
    for (const float value : g_sinkSeen) speakerSeen = speakerSeen || value == 1052.0f;
    Require(speakerSeen, "speaker grains reach the sink as speaker grains");
}

template<typename TAction>
static bool Throws(TAction action) {
    try {
        action();
    } catch (const std::invalid_argument&) {
        return true;
    }
    return false;
}

template<std::uint32_t Frames>
static std::array<float, Frames * AUDIO_OUT2_PAD_CHANNELS> Silence() {
    return {};
}

static void Accumulate(AudioOut2Route route, const float* data, std::uint32_t channels, const float* volume, float* out, std::uint32_t frames) {
    for (std::uint32_t frame = 0; frame < frames; frame++) AudioOut2AccumulatePadFrame(route, data + static_cast<std::size_t>(frame) * channels, channels, volume, out + static_cast<std::size_t>(frame) * AUDIO_OUT2_PAD_CHANNELS);
}

int main() {
    {
        const float pad[] = {1.0f, -1.0f, 0.25f, -0.5f, 1.0f, -1.0f, -0.25f, 0.5f};
        const auto rumble = AudioOut2RumbleFromPadFrames(pad, 2);
        Require(rumble[0] == 128 && rumble[1] == 255, "haptic energy drives separate motors, not speaker samples");
        const float speakersOnly[] = {1.0f, 1.0f, 0.0f, 0.0f};
        Require(AudioOut2RumbleFromPadFrames(speakersOnly, 1) == std::array<std::uint8_t, 2>{}, "speaker sound does not rumble");
        Require(AudioOut2RumbleFromPadFrames(nullptr, 0) == std::array<std::uint8_t, 2>{}, "empty input does not rumble");
    }
    Require(AudioOut2RouteForPort(0x3, 1) == AudioOut2Route::PadSpeaker, "mono type 0x3 is the pad speaker");
    Require(AudioOut2RouteForPort(0x3, 2) == AudioOut2Route::PadSpeaker, "stereo type 0x3 is the pad speaker");
    Require(AudioOut2RouteForPort(0x6, 2) == AudioOut2Route::PadVibration, "stereo type 0x6 is the vibration");
    Require(AudioOut2RouteForPort(0x6, 1) == AudioOut2Route::PadVibration, "mono type 0x6 is the vibration");
    Require(AudioOut2RouteForPort(0x6, 8) == AudioOut2Route::Main, "an 8-channel type 0x6 stays in the main mix");
    Require(AudioOut2RouteForPort(0x3, 0) == AudioOut2Route::Main, "an undecoded format stays in the main mix");
    for (std::uint16_t type : {0x0, 0x1, 0x2, 0x4, 0x5, 0x7, 0x100}) {
        Require(AudioOut2RouteForPort(type, 1) == AudioOut2Route::Main, "other port types stay in the main mix");
        Require(AudioOut2RouteForPort(type, 2) == AudioOut2Route::Main, "other port types stay in the main mix");
    }

    Require(AudioOut2IsPadAudioDevice("DualSense wireless controller (PS5) Direct DualSense Wireless Controller"), "pulse name");
    Require(AudioOut2IsPadAudioDevice("DualSense Edge Wireless Controller, USB Audio"), "edge alsa name");
    Require(!AudioOut2IsPadAudioDevice("AD102 High Definition Audio Controller Digital Stereo (HDMI)"), "hdmi name");
    Require(!AudioOut2IsPadAudioDevice(nullptr), "null name");

    constexpr std::uint32_t frames = 3;
    const float unity[2] = {1.0f, 1.0f};

    {
        auto out = Silence<frames>();
        const float speaker[frames] = {0.5f, -0.25f, 0.125f};
        const float half[1] = {0.5f};
        Accumulate(AudioOut2Route::PadSpeaker, speaker, 1, half, out.data(), frames);
        for (std::uint32_t frame = 0; frame < frames; frame++) {
            const float* pad = &out[frame * AUDIO_OUT2_PAD_CHANNELS];
            Require(pad[0] == speaker[frame] * 0.5f && pad[1] == speaker[frame] * 0.5f, "mono speaker on channels 1-2");
            Require(pad[2] == 0.0f && pad[3] == 0.0f, "speaker leaves the actuators alone");
        }
    }

    {
        auto out = Silence<frames>();
        const float vibration[frames * 2] = {0.1f, 0.2f, 0.3f, 0.4f, -0.5f, -0.6f};
        const float gains[2] = {1.0f, 0.5f};
        Accumulate(AudioOut2Route::PadVibration, vibration, 2, gains, out.data(), frames);
        for (std::uint32_t frame = 0; frame < frames; frame++) {
            const float* pad = &out[frame * AUDIO_OUT2_PAD_CHANNELS];
            Require(pad[0] == 0.0f && pad[1] == 0.0f, "vibration leaves the speaker alone");
            Require(pad[2] == vibration[frame * 2] && pad[3] == vibration[frame * 2 + 1] * 0.5f, "vibration on channels 3-4");
        }
    }

    {
        auto out = Silence<frames>();
        const float speaker[frames] = {0.75f, 0.75f, 0.0f};
        const float vibration[frames * 2] = {0.9f, -0.9f, 0.0f, 0.0f, 0.0f, 0.0f};
        Accumulate(AudioOut2Route::PadSpeaker, speaker, 1, unity, out.data(), frames);
        Accumulate(AudioOut2Route::PadSpeaker, speaker, 1, unity, out.data(), frames);
        Accumulate(AudioOut2Route::PadVibration, vibration, 2, unity, out.data(), frames);
        Accumulate(AudioOut2Route::PadVibration, vibration, 2, unity, out.data(), frames);
        AudioOut2FinishPadMix(out.data(), frames);
        Require(out[0] == 1.0f && out[1] == 1.0f, "speaker sum clamps at full scale");
        Require(out[2] == 1.0f && out[3] == -1.0f, "vibration sum clamps at full scale");
        Require(out[4] == 1.0f && out[6] == 0.0f, "second frame");
        for (std::uint32_t index = 8; index < frames * AUDIO_OUT2_PAD_CHANNELS; index++) Require(out[index] == 0.0f, "silent frame stays silent");
    }

    {
        auto out = Silence<1>();
        const float data[8] = {1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.0f, 1.0f};
        Require(Throws([&] { AudioOut2AccumulatePadFrame(AudioOut2Route::Main, data, 2, unity, out.data()); }), "a main-mix port is refused");
        Require(Throws([&] { AudioOut2AccumulatePadFrame(AudioOut2Route::PadVibration, data, 8, unity, out.data()); }), "an 8-channel pad port is refused");
        for (float sample : out) Require(sample == 0.0f, "a refused port mixes nothing");
    }

    {
        const auto quad = AudioOut2PadLayoutForDriver("pipewire");
        Require(quad.channels == 4 && quad.position[0] == 0 && quad.position[1] == 1 && quad.position[2] == 2 && quad.position[3] == 3, "quad layout");
        Require(AudioOut2PadLayoutForDriver("alsa").channels == 4 && AudioOut2PadLayoutForDriver(nullptr).channels == 4, "alsa and unknown use quad");
        const auto pulse = AudioOut2PadLayoutForDriver("pulseaudio");
        Require(pulse.channels == 6 && pulse.position[0] == 0 && pulse.position[1] == 1 && pulse.position[2] == 4 && pulse.position[3] == 5, "pulse layout");

        const float pad[2 * AUDIO_OUT2_PAD_CHANNELS] = {0.1f, 0.2f, 0.3f, 0.4f, 0.5f, 0.6f, 0.7f, 0.8f};
        float out[2 * AUDIO_OUT2_PAD_DEVICE_CHANNELS_MAX];
        for (float& sample : out) sample = 9.0f;
        AudioOut2WritePadFrames(pad, pulse, out, 2);
        const float expected[2 * 6] = {0.1f, 0.2f, 0.0f, 0.0f, 0.3f, 0.4f, 0.5f, 0.6f, 0.0f, 0.0f, 0.7f, 0.8f};
        for (int index = 0; index < 12; index++) Require(out[index] == expected[index], "pulse frames: speaker front, centre and LFE silent, actuators back");
        AudioOut2WritePadFrames(pad, quad, out, 2);
        for (int index = 0; index < 8; index++) Require(out[index] == pad[index], "quad frames are the pad mix");
    }

    {
        Require(AudioOut2PadDeviceWanted("AGS Controller 1", "AGS Controller 1"), "the named sink is used");
        Require(!AudioOut2PadDeviceWanted("DualSense Wireless Controller", "AGS Controller 1"), "a named sink excludes other pads");
        Require(!AudioOut2PadDeviceWanted(nullptr, "AGS Controller 1"), "null name with a sink");
        Require(AudioOut2PadDeviceWanted("DualSense Wireless Controller", ""), "an empty sink is the name match");
        Require(AudioOut2PadDeviceWanted("DualSense Wireless Controller", nullptr), "no sink is the name match");
        Require(!AudioOut2PadDeviceWanted("HDMI Digital Stereo", nullptr), "no sink still skips other cards");
    }

    {
        Require(AudioOut2HapticGain(2.5f) == 2.5f && AudioOut2HapticGain(0.0f) == 0.0f, "a gain in range is kept");
        Require(AudioOut2HapticGain(9.0f) == 4.0f && AudioOut2HapticGain(-1.0f) == 0.0f, "the gain is clamped to 0..4");
        Require(AudioOut2HapticGain(std::numeric_limits<float>::quiet_NaN()) == 1.0f && AudioOut2HapticGain(std::numeric_limits<float>::infinity()) == 1.0f, "a non-finite gain is 1");
        const float pad[2 * AUDIO_OUT2_PAD_CHANNELS] = {0.1f, 0.2f, 0.3f, -0.4f, 0.5f, 0.6f, 0.7f, -0.8f};
        const auto haptics = AudioOut2PadStereo(pad, 2, AUDIO_OUT2_PAD_VIBRATION_LEFT, 2.0f);
        Require(haptics.size() == 4 && haptics[0] == 0.6f && haptics[1] == -0.8f && haptics[2] == 1.0f && haptics[3] == -1.0f, "haptic channels are scaled and clipped at full scale");
        const auto speaker = AudioOut2PadStereo(pad, 2, AUDIO_OUT2_PAD_SPEAKER_LEFT, 1.0f);
        Require(speaker[0] == 0.1f && speaker[1] == 0.2f && speaker[2] == 0.5f && speaker[3] == 0.6f, "speaker channels are the first pair");
    }

    CheckPadAudioQueue();

    return 0;
}
