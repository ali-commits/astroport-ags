#ifndef CORE_LIBS_PRX_LIBKERNEL_INCLUDE_EXTERNALRENDERER_HPP
#define CORE_LIBS_PRX_LIBKERNEL_INCLUDE_EXTERNALRENDERER_HPP

#include <cstddef>
#include <cstdint>
#include "SceTypes.hpp"
#include "prx/libc/include/general/VabiMacros.hpp"

struct ExternalRenderer {
    std::uint32_t size;
    int (APS5_VABI *allocateDirectMemory)(std::int64_t searchStart, std::int64_t searchEnd, std::size_t len, std::size_t alignment, int memoryType, std::int64_t* physAddrOut);
    int (APS5_VABI *allocateMainDirectMemory)(std::size_t len, std::size_t alignment, int memoryType, std::int64_t* physAddrOut);
    int (APS5_VABI *directMemoryQuery)(std::int64_t offset, int flags, void* info, std::size_t infoSize);
    std::size_t (APS5_VABI *getDirectMemorySize)();
    int (APS5_VABI *mapDirectMemory)(void** addr, std::size_t len, int prot, int flags, std::int64_t directMemoryStart, std::size_t alignment);
    int (APS5_VABI *mapDirectMemory2)(void** addr, std::size_t len, int type, int prot, int flags, std::int64_t directMemoryStart, std::size_t alignment);
    int (APS5_VABI *mapNamedDirectMemory)(void** addr, std::size_t len, int prot, int flags, std::int64_t directMemoryStart, std::size_t alignment, const char* name);
    int (APS5_VABI *mapFlexibleMemory)(void** addr, std::size_t len, int prot, int flags);
    std::int32_t (APS5_VABI *mapNamedFlexibleMemory)(void** addr, std::size_t len, int prot, int flags, const char* name);
    int (APS5_VABI *releaseDirectMemory)(std::int64_t start, std::size_t len);
    int (APS5_VABI *availableFlexibleMemorySize)(std::size_t* size);
    int (APS5_VABI *configuredFlexibleMemorySize)(std::size_t* size);
    int (APS5_VABI *mprotect)(const void* addr, std::size_t len, int prot);
    int (APS5_VABI *munmap)(std::uint64_t addr, std::size_t len);
    int (APS5_VABI *virtualQuery)(const void* addr, int flags, VirtualQueryInfo* info, std::uint64_t infoSize);
    int (APS5_VABI *setVirtualRangeName)(const void* addr, std::uint64_t len, const char* name);
    int (APS5_VABI *clearVirtualRangeName)(const void* addr, std::uint64_t len);
    bool (*owns)(std::uintptr_t address, std::size_t bytes);
    std::uint64_t (*padAudio)(std::uint32_t rate, int speaker, const void* samples, std::uint32_t frames, std::uint32_t channels, int isFloat, float gain);
    float (*hapticGain)();
    void (*videoActive)(int active);
};

static_assert(sizeof(ExternalRenderer) == 176 && offsetof(ExternalRenderer, owns) == 144);

extern "C" const ExternalRenderer* ExternalRendererGet_nid_no_patch();
extern "C" void ExternalRendererSet_nid_no_patch(const ExternalRenderer* renderer);

#endif
