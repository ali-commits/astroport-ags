#ifndef CORE_LIBS_PRX_LIBASTROPORTHOST_LIBASTROPORTHOST_HPP
#define CORE_LIBS_PRX_LIBASTROPORTHOST_LIBASTROPORTHOST_HPP

#include <cstdint>

inline constexpr std::uint32_t AstroportHostAbi = 1;
inline constexpr int AstroportEventEop = 0;
inline constexpr int AstroportEventFlip = 1;

struct AstroportTouch {
    std::uint16_t x;
    std::uint16_t y;
    std::uint8_t id;
    std::uint8_t active;
};

struct AstroportPadInput {
    std::uint32_t buttons;
    std::uint8_t sticks[4];
    std::uint8_t l2;
    std::uint8_t r2;
    std::uint8_t connected;
    std::uint8_t hasMotion;
    AstroportTouch touch[2];
    float accel[3];
    float gyro[3];
};

struct AstroportPadOutput {
    std::uint8_t rumbleLarge;
    std::uint8_t rumbleSmall;
    std::uint8_t audioRumbleLarge;
    std::uint8_t audioRumbleSmall;
    std::uint8_t lightBarValid;
    std::uint8_t lightBar[3];
    std::uint8_t triggerSet[2];
    std::uint8_t trigger[2][56];
};

struct AstroportHostApi {
    std::uint32_t abi;
    std::uint32_t size;
    std::uint64_t (*donate)(std::uint64_t base, std::uint64_t bytes);
    int (*eventAdd)(std::uint64_t queue, int kind, std::uint64_t ident, void* userData);
    int (*eventDelete)(std::uint64_t queue, int kind, std::uint64_t ident);
    void (*eventDeliver)(int kind, std::uint64_t ident, std::uint64_t data);
    void (*padPublish)(int port, const AstroportPadInput* input);
    int (*padFetchOutput)(int port, std::uint32_t* sequence, AstroportPadOutput* out);
    void (*requestExit)(int code);
    void (*onShutdown)(void (*callback)());
    void (*installRenderer)(const void* externalRenderer);
};

static_assert(sizeof(AstroportTouch) == 6 && sizeof(AstroportPadInput) == 48 && sizeof(AstroportPadOutput) == 122 && sizeof(AstroportHostApi) == 80);

extern "C" const AstroportHostApi* AstroportHostApi_nid_no_patch();

#endif
