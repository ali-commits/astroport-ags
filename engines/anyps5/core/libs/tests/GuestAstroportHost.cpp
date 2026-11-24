#include "prx/libAstroportHost/libAstroportHost.hpp"
#include "prx/libScePad/include/PadState.hpp"
#include "prx/libc/include/GuestArena.hpp"
#include "prx/libkernel/Apr/include/AprCommandBuffer.hpp"
#include "prx/libkernel/Equeue/Equeue.hpp"
#include "prx/libkernel/KernelErrors.hpp"
#include "prx/libkernel/include/Api.hpp"
#include "prx/libkernel/include/ExternalRenderer.hpp"
#include "SceTypes.hpp"
#include <array>
#include <atomic>
#include <cerrno>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <sys/mman.h>
#include <unistd.h>

namespace {

constexpr std::uint64_t DonationBase = 0xDC00000000ull;
constexpr std::uint64_t DonationBytes = 16ull << 20;
constexpr std::uintptr_t OwnedBase = 0x5000000000ull;
constexpr std::size_t OwnedBytes = 1ull << 30;
constexpr int FakeResult = 1000;

void Require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

template <typename Body>
std::string CaptureStderr(Body&& body) {
    std::fflush(stderr);
    const int saved = ::dup(2);
    std::FILE* capture = std::tmpfile();
    Require(saved >= 0 && capture != nullptr && ::dup2(::fileno(capture), 2) >= 0, "cannot capture stderr");
    body();
    std::fflush(stderr);
    ::dup2(saved, 2);
    ::close(saved);
    std::string text(static_cast<std::size_t>(std::ftell(capture)), '\0');
    std::rewind(capture);
    text.resize(std::fread(text.data(), 1, text.size(), capture));
    std::fclose(capture);
    return text;
}

bool Unmapped(std::uint64_t address) {
    void* probe = ::mmap(reinterpret_cast<void*>(address), 4096, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (probe == MAP_FAILED) {
        Require(errno == EEXIST, "the mapping probe failed for another reason than an existing mapping");
        return false;
    }
    Require(reinterpret_cast<std::uint64_t>(probe) == address, "the kernel ignored MAP_FIXED_NOREPLACE");
    ::munmap(probe, 4096);
    return true;
}

void TestDonate(const AstroportHostApi& api) {
    Require(!Unmapped(DonationBase) && !Unmapped(DonationBase + DonationBytes - 4096), "the guest arena does not reserve the range before the donation");
    Require(api.donate(DonationBase, DonationBytes) == DonationBase, "a free donation was refused");
    Require(Unmapped(DonationBase) && Unmapped(DonationBase + DonationBytes - 4096), "the donated range is still mapped");
    Require(!Unmapped(DonationBase - 4096) && !Unmapped(DonationBase + DonationBytes), "the donation unmapped memory outside the range");
    std::uintptr_t start = 0;
    std::uintptr_t end = 0;
    Require(GuestArena::GuestArenaNextUsed_nid_no_patch(DonationBase, &start, &end) && start == DonationBase && end == DonationBase + DonationBytes, "the donated range is not marked used");
}

void TestConflict(const AstroportHostApi& api) {
    Require(api.donate(DonationBase, DonationBytes) == DonationBase, "a free donation was refused");
    std::uint64_t result = 1;
    std::string text = CaptureStderr([&] { result = api.donate(DonationBase + DonationBytes / 2, DonationBytes); });
    Require(result == 0, "a donation overlapping a donated range was accepted");
    Require(text.find("0xdc00800000..0xdc01800000") != std::string::npos && text.find("0xdc00000000..0xdc01000000 is in use") != std::string::npos, "the refusal does not name the requested and the conflicting range");
    Require(!Unmapped(DonationBase + DonationBytes), "a refused donation unmapped memory");
    text = CaptureStderr([&] { result = api.donate(0x7FFFF0000ull, 0x10000); });
    Require(result == 0 && text.find("0x7ffffc000..0x1000000000 is in use") != std::string::npos, "a donation overlapping the system-reserved range was accepted or not explained");
    text = CaptureStderr([&] { result = api.donate(DonationBase + (64ull << 20) + 1, 4096); });
    Require(result == 0 && !text.empty(), "a misaligned donation was accepted or not explained");
    text = CaptureStderr([&] { result = api.donate(0xFC00000000ull, 4096); });
    Require(result == 0 && !text.empty(), "a donation outside the guest arena was accepted or not explained");
}

int Poll(KernelEqueue queue, KernelEvent* events, int capacity) {
    int count = 0;
    const KernelUseconds poll = 0;
    const int result = sceKernelWaitEqueue(queue, events, capacity, &count, &poll);
    if (result == SCE_KERNEL_ERROR_ETIMEDOUT) return 0;
    Require(result == 0, "the event queue wait failed");
    return count;
}

void TestEvents(const AstroportHostApi& api) {
    constexpr std::int16_t EvfiltGraphicsCore = -14;
    constexpr std::uint64_t First = 0x1122334455667788ull;
    constexpr std::uint64_t Second = 0xFFEEDDCCBBAA9988ull;
    KernelEqueue queue = 0;
    KernelEqueue other = 0;
    KernelEqueue gone = 0;
    Require(sceKernelCreateEqueue(&queue, "gpu") == 0 && sceKernelCreateEqueue(&other, "other") == 0 && sceKernelCreateEqueue(&gone, "gone") == 0, "cannot create the event queues");
    const auto handle = static_cast<std::uint64_t>(queue);
    const auto otherHandle = static_cast<std::uint64_t>(other);
    const auto goneHandle = static_cast<std::uint64_t>(gone);
    int eopTag = 0;
    int flipTag = 0;
    int otherTag = 0;
    std::array<KernelEvent, 32> events{};

    Require(api.eventAdd(0, AstroportEventEop, 5, &eopTag) == SCE_KERNEL_ERROR_EBADF, "a zero queue was accepted");
    Require(api.eventAdd(handle + otherHandle + 1000, AstroportEventEop, 5, &eopTag) == SCE_KERNEL_ERROR_EBADF, "an unknown queue was accepted");
    Require(api.eventAdd(handle, 2, 5, &eopTag) == SCE_KERNEL_ERROR_EBADF, "an unknown event kind was accepted");
    Require(api.eventDelete(handle, 2, 5) == SCE_KERNEL_ERROR_EBADF, "an unknown event kind was deleted");
    Require(api.eventDelete(handle, AstroportEventEop, 5) == SCE_KERNEL_ERROR_ENOENT, "deleting an unregistered event did not return the queue's error");

    Require(api.eventAdd(goneHandle, AstroportEventEop, 7, &eopTag) == 0 && api.eventAdd(goneHandle, AstroportEventFlip, 3, &flipTag) == 0, "the registrations on the queue to delete failed");
    Require(api.eventAdd(handle, AstroportEventEop, 5, &eopTag) == 0, "the EOP registration failed");
    Require(api.eventAdd(handle, AstroportEventEop, 5, &eopTag) == 0, "a repeated EOP registration failed");
    api.eventDeliver(AstroportEventEop, 6, First);
    Require(Poll(queue, events.data(), 32) == 0, "an EOP interrupt reached a registration with another ident");
    api.eventDeliver(AstroportEventEop, 5, First);
    api.eventDeliver(AstroportEventEop, 5, First);
    Require(Poll(queue, events.data(), 32) == 1, "two EOP interrupts did not coalesce into one event");
    Require(events[0].ident == 5 && events[0].filter == EvfiltGraphicsCore && events[0].flags == (EV_ADD | EV_CLEAR) && events[0].data == 2 && events[0].udata == &eopTag, "the EOP event is wrong");
    Require(Poll(queue, events.data(), 32) == 0, "the EOP event was not cleared");
    api.eventDeliver(AstroportEventEop, 5, First);
    Require(Poll(queue, events.data(), 32) == 1 && events[0].data == 1, "the EOP count did not restart");

    Require(api.eventAdd(handle, AstroportEventFlip, 1, &flipTag) == 0, "the flip registration failed");
    Require(api.eventAdd(handle, AstroportEventFlip, 2, &flipTag) == 0, "a second flip registration on the same queue failed");
    Require(api.eventAdd(otherHandle, AstroportEventFlip, 9, &otherTag) == 0, "the flip registration on the other queue failed");
    api.eventDeliver(AstroportEventFlip, 77, First);
    api.eventDeliver(AstroportEventFlip, 77, Second);
    Require(Poll(queue, events.data(), 32) == 2, "a flip behind a still-triggered flip was not queued");
    Require(events[0].ident == 0 && events[0].filter == EVFILT_VIDEO_OUT && events[0].flags == EV_ADD && events[0].udata == &flipTag, "the flip event is wrong");
    Require(events[0].fflags == 1 && static_cast<std::uint64_t>(events[0].data) == First, "the first flip payload or counter is wrong");
    Require(events[1].fflags == 2 && static_cast<std::uint64_t>(events[1].data) == Second, "the queued flip payload or counter is wrong");
    Require(Poll(other, events.data(), 32) == 2 && events[0].udata == &otherTag && static_cast<std::uint64_t>(events[1].data) == Second, "the flip did not reach every flip registration");
    Require(Poll(queue, events.data(), 32) == 0, "the flip event was not reset");
    for (int index = 0; index < 20; ++index) api.eventDeliver(AstroportEventFlip, 1, static_cast<std::uint64_t>(index));
    Require(Poll(queue, events.data(), 32) == 20 && events[0].fflags == 1 && events[0].data == 0 && events[19].fflags == 2 && events[19].data == 19, "queued flips were lost, reordered or miscounted");
    Require(Poll(other, events.data(), 32) == 20, "the other queue lost flips");

    Require(api.eventDelete(handle, AstroportEventEop, 5) == 0 && api.eventDelete(handle, AstroportEventFlip, 1) == 0, "deleting a registered event failed");
    Require(api.eventDelete(handle, AstroportEventFlip, 1) == SCE_KERNEL_ERROR_ENOENT, "deleting a deleted event did not return the queue's error");
    api.eventDeliver(AstroportEventEop, 5, First);
    api.eventDeliver(AstroportEventFlip, 1, First);
    Require(Poll(queue, events.data(), 32) == 0, "a deleted registration still received events");
    Require(Poll(other, events.data(), 32) == 1, "deleting one registration removed another");
    Require(api.eventAdd(otherHandle, AstroportEventEop, 7, &otherTag) == 0, "the EOP registration on the other queue failed");
    Require(sceKernelDeleteEqueue(gone) == 0 && sceKernelDeleteEqueue(queue) == 0, "cannot delete the event queues");
    api.eventDeliver(AstroportEventEop, 7, First);
    api.eventDeliver(AstroportEventFlip, 1, First);
    Require(Poll(other, events.data(), 32) == 2 && events[0].filter + events[1].filter == EvfiltGraphicsCore + EVFILT_VIDEO_OUT, "a registration on a deleted queue stopped the delivery to the other queue");
    Require(api.eventDelete(goneHandle, AstroportEventFlip, 3) == SCE_KERNEL_ERROR_EBADF, "deleting a registration of a deleted queue did not return the queue's error");
}

std::array<int, 17> g_calls{};
std::array<std::uint64_t, 17> g_arguments{};

template <typename... Values>
std::uint64_t Fold(Values... values) {
    std::uint64_t folded = 0;
    ((folded = folded * 1000003u + static_cast<std::uint64_t>(values)), ...);
    return folded;
}

template <typename... Values>
int Hit(int index, Values... values) {
    ++g_calls[static_cast<std::size_t>(index)];
    g_arguments[static_cast<std::size_t>(index)] = Fold(reinterpret_cast<std::uint64_t>(values)...);
    return FakeResult + index;
}

template <typename Value>
void* P(Value value) {
    return reinterpret_cast<void*>(static_cast<std::uintptr_t>(value));
}

bool Owns(std::uintptr_t address, std::size_t bytes) {
    return address >= OwnedBase && bytes <= OwnedBytes && address - OwnedBase <= OwnedBytes - bytes;
}

ExternalRenderer FakeRenderer() {
    ExternalRenderer renderer{};
    renderer.size = sizeof(ExternalRenderer);
    renderer.allocateDirectMemory = [](std::int64_t a, std::int64_t b, std::size_t c, std::size_t d, int e, std::int64_t* f) { return Hit(0, P(a), P(b), P(c), P(d), P(e), f); };
    renderer.allocateMainDirectMemory = [](std::size_t a, std::size_t b, int c, std::int64_t* d) { return Hit(1, P(a), P(b), P(c), d); };
    renderer.directMemoryQuery = [](std::int64_t a, int b, void* c, std::size_t d) { return Hit(2, P(a), P(b), c, P(d)); };
    renderer.getDirectMemorySize = []() { return static_cast<std::size_t>(Hit(3)); };
    renderer.mapDirectMemory = [](void** a, std::size_t b, int c, int d, std::int64_t e, std::size_t f) { return Hit(4, a, P(b), P(c), P(d), P(e), P(f)); };
    renderer.mapDirectMemory2 = [](void** a, std::size_t b, int c, int d, int e, std::int64_t f, std::size_t g) { return Hit(5, a, P(b), P(c), P(d), P(e), P(f), P(g)); };
    renderer.mapNamedDirectMemory = [](void** a, std::size_t b, int c, int d, std::int64_t e, std::size_t f, const char* g) { return Hit(6, a, P(b), P(c), P(d), P(e), P(f), g); };
    renderer.mapFlexibleMemory = [](void** a, std::size_t b, int c, int d) { return Hit(7, a, P(b), P(c), P(d)); };
    renderer.mapNamedFlexibleMemory = [](void** a, std::size_t b, int c, int d, const char* e) { return static_cast<std::int32_t>(Hit(8, a, P(b), P(c), P(d), e)); };
    renderer.releaseDirectMemory = [](std::int64_t a, std::size_t b) { return Hit(9, P(a), P(b)); };
    renderer.availableFlexibleMemorySize = [](std::size_t* a) { return Hit(10, a); };
    renderer.configuredFlexibleMemorySize = [](std::size_t* a) { return Hit(11, a); };
    renderer.mprotect = [](const void* a, std::size_t b, int c) { return Hit(12, a, P(b), P(c)); };
    renderer.munmap = [](std::uint64_t a, std::size_t b) { return Hit(13, P(a), P(b)); };
    renderer.virtualQuery = [](const void* a, int b, VirtualQueryInfo* c, std::uint64_t d) { return Hit(14, a, P(b), c, P(d)); };
    renderer.setVirtualRangeName = [](const void* a, std::uint64_t b, const char* c) { return Hit(15, a, P(b), c); };
    renderer.clearVirtualRangeName = [](const void* a, std::uint64_t b) { return Hit(16, a, P(b)); };
    renderer.owns = Owns;
    return renderer;
}

void Forwarded(int index, std::int64_t result, std::uint64_t arguments, const char* message) {
    const auto slot = static_cast<std::size_t>(index);
    Require(result == FakeResult + index && g_calls[slot] == 1 && g_arguments[slot] == arguments, message);
    g_calls[slot] = 0;
}

void TestRenderer(const AstroportHostApi& api) {
    const std::size_t upstreamDirectMemory = sceKernelGetDirectMemorySize();
    Require(ExternalRendererGet_nid_no_patch() == nullptr && upstreamDirectMemory != static_cast<std::size_t>(FakeResult + 3), "a renderer is installed before the test installs one");
    bool rejected = false;
    try {
        const ExternalRenderer small{};
        ExternalRendererSet_nid_no_patch(&small);
    } catch (const std::invalid_argument&) {
        rejected = true;
    }
    Require(rejected && ExternalRendererGet_nid_no_patch() == nullptr, "a table smaller than the structure was installed");

    static const ExternalRenderer renderer = FakeRenderer();
    api.installRenderer(&renderer);
    Require(ExternalRendererGet_nid_no_patch() == &renderer, "the renderer table was not installed");

    std::int64_t physical = 0;
    void* address = nullptr;
    std::size_t size = 0;
    VirtualQueryInfo info{};
    const char* name = "name";
    auto* const owned = reinterpret_cast<void*>(OwnedBase + 0x4000);
    auto* const foreign = reinterpret_cast<void*>(OwnedBase - 0x4000);
    const auto ownedValue = reinterpret_cast<std::uint64_t>(owned);
    const auto u = [](const void* pointer) { return reinterpret_cast<std::uint64_t>(pointer); };

    Forwarded(0, sceKernelAllocateDirectMemory(1, 2, 3, 4, 5, &physical), Fold(1, 2, 3, 4, 5, u(&physical)), "sceKernelAllocateDirectMemory was not forwarded");
    Forwarded(1, sceKernelAllocateMainDirectMemory(6, 7, 8, &physical), Fold(6, 7, 8, u(&physical)), "sceKernelAllocateMainDirectMemory was not forwarded");
    Forwarded(2, sceKernelDirectMemoryQuery(9, 10, &info, 11), Fold(9, 10, u(&info), 11), "sceKernelDirectMemoryQuery was not forwarded");
    Forwarded(3, static_cast<std::int64_t>(sceKernelGetDirectMemorySize()), 0, "sceKernelGetDirectMemorySize was not forwarded");
    Forwarded(4, sceKernelMapDirectMemory(&address, 12, 13, 14, 15, 16), Fold(u(&address), 12, 13, 14, 15, 16), "sceKernelMapDirectMemory was not forwarded");
    Forwarded(5, sceKernelMapDirectMemory2(&address, 17, 18, 19, 20, 21, 22), Fold(u(&address), 17, 18, 19, 20, 21, 22), "sceKernelMapDirectMemory2 was not forwarded");
    Forwarded(6, sceKernelMapNamedDirectMemory(&address, 23, 24, 25, 26, 27, name), Fold(u(&address), 23, 24, 25, 26, 27, u(name)), "sceKernelMapNamedDirectMemory was not forwarded");
    Forwarded(7, sceKernelMapFlexibleMemory(&address, 28, 29, 30), Fold(u(&address), 28, 29, 30), "sceKernelMapFlexibleMemory was not forwarded");
    Forwarded(8, sceKernelMapNamedFlexibleMemory(&address, 31, 32, 33, name), Fold(u(&address), 31, 32, 33, u(name)), "sceKernelMapNamedFlexibleMemory was not forwarded");
    Forwarded(9, sceKernelReleaseDirectMemory(34, 35), Fold(34, 35), "sceKernelReleaseDirectMemory was not forwarded");
    Forwarded(10, sceKernelAvailableFlexibleMemorySize(&size), u(&size), "sceKernelAvailableFlexibleMemorySize was not forwarded");
    Forwarded(11, sceKernelConfiguredFlexibleMemorySize(&size), u(&size), "sceKernelConfiguredFlexibleMemorySize was not forwarded");

    Forwarded(12, sceKernelMprotect(owned, 0x4000, 3), Fold(ownedValue, 0x4000, 3), "sceKernelMprotect on renderer memory was not forwarded");
    Forwarded(12, sceKernelMprotect(owned, 0, 3), Fold(ownedValue, 0, 3), "a zero-length sceKernelMprotect on renderer memory was not forwarded");
    Forwarded(13, sceKernelMunmap(ownedValue, 0x8000), Fold(ownedValue, 0x8000), "sceKernelMunmap on renderer memory was not forwarded");
    Forwarded(14, sceKernelVirtualQuery(owned, 1, &info, sizeof(info)), Fold(ownedValue, 1, u(&info), sizeof(info)), "sceKernelVirtualQuery on renderer memory was not forwarded");
    Forwarded(15, sceKernelSetVirtualRangeName(owned, 0x4000, name), Fold(ownedValue, 0x4000, u(name)), "sceKernelSetVirtualRangeName on renderer memory was not forwarded");
    Forwarded(16, sceKernelClearVirtualRangeName(owned, 0x4000), Fold(ownedValue, 0x4000), "sceKernelClearVirtualRangeName on renderer memory was not forwarded");

    Require(sceKernelMprotect(foreign, 0, 3) == 0, "sceKernelMprotect outside renderer memory was forwarded");
    Require(sceKernelMunmap(u(foreign), 0) == SCE_KERNEL_ERROR_EINVAL, "sceKernelMunmap outside renderer memory was forwarded");
    Require(sceKernelVirtualQuery(foreign, 0, nullptr, 0) == SCE_KERNEL_ERROR_EINVAL, "sceKernelVirtualQuery outside renderer memory was forwarded");
    Require(sceKernelSetVirtualRangeName(foreign, 0, name) == SCE_KERNEL_ERROR_EINVAL, "sceKernelSetVirtualRangeName outside renderer memory was forwarded");
    Require(sceKernelClearVirtualRangeName(foreign, 0) == SCE_KERNEL_ERROR_EINVAL, "sceKernelClearVirtualRangeName outside renderer memory was forwarded");
    Require(sceKernelClearVirtualRangeName(foreign, 0x8000) == 0, "sceKernelClearVirtualRangeName on a range that leaves renderer memory was forwarded");
    for (const int calls : g_calls) Require(calls == 0, "a call outside renderer memory reached the renderer");

    const std::string text = CaptureStderr([&] {
        Require(sceKernelMtypeprotect(owned, 0, 0, 3) == 0 && sceKernelMtypeprotect(owned, 0, 0, 3) == 0, "a zero-length sceKernelMtypeprotect failed");
        Require(sceKernelMtypeprotect(foreign, 0, 0, 3) == 0, "a zero-length sceKernelMtypeprotect failed");
    });
    const auto line = text.find("unforwarded memory call sceKernelMtypeprotect on renderer memory 0x5000004000+0x0");
    Require(line != std::string::npos && text.find("unforwarded memory call", line + 1) == std::string::npos, "an unforwarded call on renderer memory was not reported exactly once");

    auto* const misaligned = reinterpret_cast<void*>(OwnedBase + 0x4001);
    void* reserve = owned;
    void* reserveForeign = foreign;
    KernelBatchMapEntry entry{};
    entry.start = owned;
    entry.operation = 2;
    KernelBatchMapEntry unmapEntry{};
    unmapEntry.start = foreign;
    unmapEntry.operation = 1;
    int processed = 0;
    const std::string others = CaptureStderr([&] {
        for (int round = 0; round < 2; ++round) {
            Require(sceKernelMmap(owned, 0, 0, 0, -1, 0, &address) != 0, "a zero-length sceKernelMmap succeeded");
            Require(sceKernelReleaseFlexibleMemory(misaligned, 0x4000) == SCE_KERNEL_ERROR_EINVAL, "a misaligned sceKernelReleaseFlexibleMemory succeeded");
            Require(sceKernelReserveVirtualRange(&reserve, 0, 0, 0) == SCE_KERNEL_ERROR_EINVAL, "a zero-length sceKernelReserveVirtualRange succeeded");
            Require(sceKernelBatchMap2(&entry, 1, &processed, 0) == SCE_KERNEL_ERROR_EINVAL, "a zero-length sceKernelBatchMap2 entry succeeded");
        }
        Require(sceKernelReserveVirtualRange(&reserveForeign, 0, 0, 0) == SCE_KERNEL_ERROR_EINVAL, "a zero-length sceKernelReserveVirtualRange succeeded");
        Require(sceKernelBatchMap2(&unmapEntry, 1, &processed, 0) == SCE_KERNEL_ERROR_EINVAL, "a zero-length sceKernelBatchMap2 entry succeeded");
    });
    std::size_t lines = 0;
    for (auto at = others.find("unforwarded memory call"); at != std::string::npos; at = others.find("unforwarded memory call", at + 1)) ++lines;
    Require(lines == 4, "the other unforwarded calls on renderer memory were not reported exactly once each");
    Require(others.find("sceKernelMmap on renderer memory 0x5000004000+0x0") != std::string::npos, "sceKernelMmap on renderer memory was not reported");
    Require(others.find("sceKernelReleaseFlexibleMemory on renderer memory 0x5000004001+0x4000") != std::string::npos, "sceKernelReleaseFlexibleMemory on renderer memory was not reported");
    Require(others.find("sceKernelReserveVirtualRange on renderer memory 0x5000004000+0x0") != std::string::npos, "sceKernelReserveVirtualRange on renderer memory was not reported");
    Require(others.find("sceKernelBatchMap2 on renderer memory 0x5000004000+0x0") != std::string::npos, "sceKernelBatchMap2 on renderer memory was not reported");

    api.installRenderer(nullptr);
    Require(sceKernelGetDirectMemorySize() == upstreamDirectMemory && g_calls[3] == 0, "removing the table did not restore libkernel's own memory functions");
}

constexpr std::size_t ReadBytes = 256u << 10;
std::atomic<int> g_faults{0};
std::uint8_t* g_protected = nullptr;

void UnprotectOnWrite(int, siginfo_t* info, void*) {
    const auto address = reinterpret_cast<std::uintptr_t>(info->si_addr);
    const auto begin = reinterpret_cast<std::uintptr_t>(g_protected);
    if (address < begin || address - begin >= ReadBytes) {
        std::signal(SIGSEGV, SIG_DFL);
        return;
    }
    ::mprotect(reinterpret_cast<void*>(address & ~std::uintptr_t{4095}), 4096, PROT_READ | PROT_WRITE);
    g_faults.fetch_add(1);
}

std::uint8_t FileByte(std::size_t offset) {
    return static_cast<std::uint8_t>((offset * 37u) ^ (offset >> 8u));
}

void TestApr(const AstroportHostApi& api) {
    constexpr const char* FilePath = "astroport_host_apr.bin";
    {
        std::ofstream stream(FilePath, std::ios::binary | std::ios::trunc);
        for (std::size_t offset = 0; offset < ReadBytes; ++offset) stream.put(static_cast<char>(FileByte(offset)));
        stream.close();
        Require(!stream.fail(), "cannot create the APR file");
    }
    const char* path = FilePath;
    std::uint32_t id = 0;
    std::uint32_t failed = 0;
    Require(sceKernelAprResolveFilepathsToIds(&path, 1, &id, &failed) == 0 && failed == 0, "cannot resolve the APR file");
    g_protected = static_cast<std::uint8_t*>(::mmap(nullptr, ReadBytes, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0));
    Require(g_protected != MAP_FAILED, "cannot map the protected destination");
    struct sigaction action {};
    action.sa_sigaction = UnprotectOnWrite;
    action.sa_flags = SA_SIGINFO;
    Require(::sigaction(SIGSEGV, &action, nullptr) == 0, "cannot install the write fault handler");
    const auto read = [&] {
        const Apr::ReadFileCommand command{{Apr::Opcode::ReadFile, sizeof(Apr::ReadFileCommand)}, id, 0, reinterpret_cast<std::uint64_t>(g_protected), ReadBytes, 0};
        alignas(8) std::array<std::uint8_t, sizeof(command)> memory{};
        std::memcpy(memory.data(), &command, sizeof(command));
        Apr::CommandBufferObject buffer{Apr::BufferType::Generic, 0, sizeof(memory), 1, sizeof(memory), memory.data()};
        Require(sceKernelAprSubmitCommandBuffer(&buffer, 0) == 0, "the APR read submission failed");
    };

    bool rejected = false;
    try {
        read();
    } catch (const std::runtime_error&) {
        rejected = true;
    }
    Require(rejected && g_faults.load() == 0, "without a renderer the read into a write-protected page did not fail in the kernel");

    static ExternalRenderer renderer{};
    renderer.size = sizeof(ExternalRenderer);
    renderer.owns = Owns;
    api.installRenderer(&renderer);
    read();
    api.installRenderer(nullptr);
    Require(g_faults.load() == static_cast<int>(ReadBytes / 4096), "the staged read did not fault once on every protected page");
    for (std::size_t offset = 0; offset < ReadBytes; ++offset) Require(g_protected[offset] == FileByte(offset), "the staged read returned wrong bytes");
    std::remove(FilePath);
}

void TestPad(const AstroportHostApi& api) {
    Pad::Initialize();
    AstroportPadInput input{};
    input.buttons = 0x4010;
    input.sticks[0] = 1;
    input.sticks[1] = 2;
    input.sticks[2] = 3;
    input.sticks[3] = 4;
    input.l2 = 5;
    input.r2 = 6;
    input.connected = 1;
    input.touch[1] = {700, 300, 9, 1};
    api.padPublish(0, &input);
    const PadData data = Pad::ReadState(0);
    Require(data.buttons == 0x4010 && data.left_stick_x == 1 && data.left_stick_y == 2 && data.right_stick_x == 3 && data.right_stick_y == 4, "the published buttons or sticks are wrong");
    Require(data.analog_buttons_l2 == 5 && data.analog_buttons_r2 == 6 && data.connected, "the published triggers or connection are wrong");
    Require(data.touch_data_touch_num == 1 && (data.touch_data_touch0_x == 700 || data.touch_data_touch1_x == 700), "the published touch point is wrong");
    input.connected = 0;
    api.padPublish(0, &input);
    Require(!Pad::IsConnected(0), "the disconnection was not published");

    std::uint32_t sequence = 0;
    AstroportPadOutput output{};
    api.padFetchOutput(0, &sequence, &output);
    Require(api.padFetchOutput(0, &sequence, &output) == 0, "unchanged pad output was reported as changed");
    Pad::SetVibration(0, 10, 20);
    Pad::SetLightBar(0, true, 1, 2, 3);
    Require(api.padFetchOutput(0, &sequence, &output) == 1, "changed pad output was not reported");
    Require(output.rumbleLarge == 10 && output.rumbleSmall == 20 && output.lightBarValid == 1 && output.lightBar[0] == 1 && output.lightBar[1] == 2 && output.lightBar[2] == 3, "the fetched pad output is wrong");
    Require(api.padFetchOutput(9, &sequence, &output) == 0, "an invalid port reported output");
}

}

int main(int argc, char** argv) {
    try {
        Require(argc == 2, "expected donate, conflict, events, renderer, apr or pad");
        const AstroportHostApi* api = AstroportHostApi_nid_no_patch();
        Require(api != nullptr && api->abi == AstroportHostAbi && api->size == sizeof(AstroportHostApi), "the host table has the wrong version or size");
        const std::string mode = argv[1];
        if (mode == "donate") TestDonate(*api);
        else if (mode == "conflict") TestConflict(*api);
        else if (mode == "events") TestEvents(*api);
        else if (mode == "renderer") TestRenderer(*api);
        else if (mode == "apr") TestApr(*api);
        else if (mode == "pad") TestPad(*api);
        else throw std::runtime_error("unknown case");
        std::puts("AstroportHost checks passed");
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "%s\n", error.what());
        return 1;
    }
}
