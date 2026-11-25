#include "prx/libAstroportHost/libAstroportHost.hpp"

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <mutex>
#include <stdexcept>
#include <system_error>
#include <vector>
#include <sys/mman.h>
#include "SceTypes.hpp"
#include "prx/libSceAgcDriver/Execution/include/HostHdr.hpp"
#include "prx/libScePad/include/PadState.hpp"
#include "prx/libc/include/GuestArena.hpp"
#include "prx/libc/include/Shutdown.hpp"
#include "prx/libkernel/Equeue/Equeue.hpp"
#include "prx/libkernel/KernelErrors.hpp"
#include "prx/libkernel/include/ExternalRenderer.hpp"

namespace {

constexpr int16_t EvfiltGraphicsCore = -14;
constexpr std::uintptr_t HostPageMask = 0xFFF;

struct Registration {
    KernelEqueue queue;
    int kind;
    std::uintptr_t ident;

    bool operator==(const Registration&) const = default;
};

std::mutex g_mutex;
std::atomic<float> g_reportedToneMapPeak{0.0f};

std::vector<Registration>& Registrations() {
    static auto& registrations = *new std::vector<Registration>();
    return registrations;
}

template <typename Result, typename Body>
Result Guard(const char* name, Result failure, Body&& body) noexcept {
    try {
        return body();
    } catch (const std::exception& error) {
        std::fprintf(stderr, "[astroport-host] %s: %s\n", name, error.what());
    } catch (...) {
        std::fprintf(stderr, "[astroport-host] %s: unknown failure\n", name);
    }
    return failure;
}

template <typename Body>
void Guard(const char* name, Body&& body) noexcept {
    Guard(name, 0, [&] {
        body();
        return 0;
    });
}

unsigned long long Hex(std::uint64_t value) {
    return static_cast<unsigned long long>(value);
}

std::uint64_t Donate(std::uint64_t base, std::uint64_t bytes) {
    return Guard(__func__, std::uint64_t{0}, [&]() -> std::uint64_t {
        std::uintptr_t arena = 0;
        std::size_t arenaBytes = 0;
        GuestArena::GuestArenaRange_nid_postfix(&arena, &arenaBytes);
        if (bytes == 0 || ((base | bytes) & HostPageMask) != 0 || base < arena || bytes > arenaBytes || base - arena > arenaBytes - bytes) {
            std::fprintf(stderr, "[astroport-host] donate 0x%llx..0x%llx refused: not a page-aligned part of the guest arena 0x%llx..0x%llx\n", Hex(base), Hex(base + bytes), Hex(arena), Hex(arena + arenaBytes));
            return 0;
        }
        std::uintptr_t usedStart = 0;
        std::uintptr_t usedEnd = 0;
        if (GuestArena::GuestArenaNextUsed_nid_no_patch(base, &usedStart, &usedEnd) && usedStart < base + bytes) {
            std::fprintf(stderr, "[astroport-host] donate 0x%llx..0x%llx refused: 0x%llx..0x%llx is in use\n", Hex(base), Hex(base + bytes), Hex(usedStart), Hex(usedEnd));
            return 0;
        }
        GuestArena::GuestArenaMarkUsed_nid_postfix(reinterpret_cast<const void*>(base), bytes);
        if (::munmap(reinterpret_cast<void*>(base), bytes) != 0) throw std::system_error(errno, std::generic_category(), "munmap of the donated range");
        return base;
    });
}

bool KnownKind(int kind) {
    return kind == AstroportEventEop || kind == AstroportEventFlip;
}

std::int16_t Filter(int kind) {
    return kind == AstroportEventEop ? EvfiltGraphicsCore : EVFILT_VIDEO_OUT;
}

Registration Describe(std::uint64_t queue, int kind, std::uint64_t ident) {
    return {static_cast<KernelEqueue>(queue), kind, kind == AstroportEventEop ? static_cast<std::uintptr_t>(ident) : 0};
}

KernelEqueueEvent EopEvent() {
    KernelEqueueEvent event{};
    event.event.filter = EvfiltGraphicsCore;
    event.event.flags = EV_ADD | EV_CLEAR;
    event.filter.triggerFunc = [](KernelEqueueEvent* e, void*) {
        e->event.data = e->triggered ? e->event.data + 1 : 1;
        e->triggered = true;
    };
    event.filter.resetFunc = [](KernelEqueueEvent* e) {
        e->triggered = false;
        e->event.data = 0;
    };
    return event;
}

KernelEqueueEvent FlipEvent() {
    KernelEqueueEvent event{};
    event.event.filter = EVFILT_VIDEO_OUT;
    event.event.flags = EV_ADD;
    event.filter.triggerFunc = [](KernelEqueueEvent* e, void* data) {
        KernelEvent triggered = e->event;
        triggered.fflags = triggered.fflags < 0xFu ? triggered.fflags + 1u : triggered.fflags;
        triggered.data = static_cast<std::intptr_t>(reinterpret_cast<std::uintptr_t>(data));
        if (e->triggered) {
            e->pendingEvents.push_back(triggered);
        } else {
            e->event = triggered;
            e->triggered = true;
        }
    };
    event.filter.resetFunc = [](KernelEqueueEvent* e) {
        e->triggered = false;
        e->event.fflags = 0;
        e->event.data = 0;
    };
    return event;
}

int EventAdd(std::uint64_t queue, int kind, std::uint64_t ident, void* userData) {
    return Guard(__func__, SCE_KERNEL_ERROR_EBADF, [&] {
        const auto registration = Describe(queue, kind, ident);
        if (!KnownKind(kind) || !EqueuePin_nid_postfix(registration.queue)) return SCE_KERNEL_ERROR_EBADF;
        auto event = kind == AstroportEventEop ? EopEvent() : FlipEvent();
        event.event.ident = registration.ident;
        event.event.udata = userData;
        const int result = EqueueAddEvent_nid_postfix(registration.queue, event);
        if (result != EQUEUE_OK) return result;
        std::lock_guard lock(g_mutex);
        auto& registrations = Registrations();
        if (std::find(registrations.begin(), registrations.end(), registration) == registrations.end()) registrations.push_back(registration);
        return 0;
    });
}

int EventDelete(std::uint64_t queue, int kind, std::uint64_t ident) {
    return Guard(__func__, SCE_KERNEL_ERROR_EBADF, [&] {
        if (!KnownKind(kind)) return SCE_KERNEL_ERROR_EBADF;
        const auto registration = Describe(queue, kind, ident);
        {
            std::lock_guard lock(g_mutex);
            std::erase(Registrations(), registration);
        }
        return EqueueDeleteEvent_nid_postfix(registration.queue, registration.ident, Filter(kind));
    });
}

void EventDeliver(int kind, std::uint64_t ident, std::uint64_t data) {
    Guard(__func__, [&] {
        std::vector<Registration> registrations;
        {
            std::lock_guard lock(g_mutex);
            registrations = Registrations();
        }
        for (const auto& registration : registrations) {
            if (registration.kind != kind || (kind == AstroportEventEop && registration.ident != ident)) continue;
            EqueueTriggerEvent_nid_postfix(registration.queue, registration.ident, Filter(kind), reinterpret_cast<void*>(static_cast<std::uintptr_t>(data)));
        }
    });
}

void PadPublish(int port, const AstroportPadInput* input) {
    Guard(__func__, [&] {
        PadInputState state;
        state.buttons = input->buttons;
        std::copy(std::begin(input->sticks), std::end(input->sticks), state.sticks.begin());
        state.analogButtonsL2 = input->l2;
        state.analogButtonsR2 = input->r2;
        for (std::size_t index = 0; index < state.touch.size(); ++index) {
            const auto& touch = input->touch[index];
            state.touch[index] = {touch.active != 0, touch.x, touch.y, touch.id};
        }
        if (input->hasMotion != 0) {
            state.hasMotion = true;
            std::copy(std::begin(input->accel), std::end(input->accel), state.accel.begin());
            std::copy(std::begin(input->gyro), std::end(input->gyro), state.gyro.begin());
        }
        PadSetPortConnected_nid_postfix(port, input->connected != 0);
        PadPublishInputForPort_nid_postfix(port, state);
    });
}

int PadFetchOutput(int port, std::uint32_t* sequence, AstroportPadOutput* out) {
    return Guard(__func__, 0, [&] {
        PadOutputState state;
        if (!PadFetchOutputForPort_nid_postfix(port, sequence, &state)) return 0;
        *out = {};
        out->rumbleLarge = state.vibrationLarge;
        out->rumbleSmall = state.vibrationSmall;
        out->audioRumbleLarge = state.audioVibrationLarge;
        out->audioRumbleSmall = state.audioVibrationSmall;
        out->lightBarValid = state.lightBarValid ? 1 : 0;
        std::copy(std::begin(state.lightBar), std::end(state.lightBar), std::begin(out->lightBar));
        for (int trigger = 0; trigger < 2; ++trigger) {
            out->triggerSet[trigger] = state.trigger[trigger].set ? 1 : 0;
            std::copy(std::begin(state.trigger[trigger].command), std::end(state.trigger[trigger].command), std::begin(out->trigger[trigger]));
        }
        return 1;
    });
}

void RequestExit(int code) {
    Guard(__func__, [&] { LibcRequestExit_nid_postfix(code); });
}

void OnShutdown(void (*callback)()) {
    Guard(__func__, [&] { LibcRegisterShutdown_nid_postfix(callback); });
}

void InstallRenderer(const void* externalRenderer) {
    const bool installed = Guard(__func__, false, [&] {
        ExternalRendererSet_nid_no_patch(static_cast<const ExternalRenderer*>(externalRenderer));
        return true;
    });
    if (!installed) std::abort();
}

constexpr AstroportHostApi g_api{AstroportHostAbi, sizeof(AstroportHostApi), Donate, EventAdd, EventDelete, EventDeliver, PadPublish, PadFetchOutput, RequestExit, OnShutdown, InstallRenderer};

}

extern "C" const AstroportHostApi* AstroportHostApi_nid_no_patch() {
    return &g_api;
}

extern "C" void AgcDriverNoteReportedToneMapPeak_nid_postfix(float nits) {
    g_reportedToneMapPeak.store(nits, std::memory_order_relaxed);
}

extern "C" void AgcDriverHostHdr_nid_postfix(AgcDriver::HostHdrStatus& status) {
    status = {};
    status.reportedToneMapPeak = g_reportedToneMapPeak.load(std::memory_order_relaxed);
}
