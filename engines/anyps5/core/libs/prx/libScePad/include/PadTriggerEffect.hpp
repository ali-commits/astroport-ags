#ifndef CORE_LIBS_PRX_LIBSCEPAD_PADTRIGGEREFFECT_HPP
#define CORE_LIBS_PRX_LIBSCEPAD_PADTRIGGEREFFECT_HPP

#include <cstddef>
#include <cstdint>
#include <cstring>

inline constexpr std::size_t kPadTriggerCommandSize = 56;

inline std::int32_t PadTriggerEffectState(const std::uint8_t* command, std::uint8_t travel) {
    std::uint32_t mode = 0;
    std::memcpy(&mode, command, sizeof(mode));
    const std::uint8_t* p = command + 8;
    const bool pressed = travel >= 8;
    const int position = travel * 10 / 256;
    switch (mode) {
        case 1: return pressed && p[1] != 0 && position >= p[0] ? 2 : 1;
        case 2: return position >= p[1] ? 5 : position >= p[0] ? 4 : 3;
        case 3: return pressed && p[1] != 0 && p[2] != 0 && position >= p[0] ? 7 : 6;
        case 4: return pressed && p[position] != 0 ? 2 : 1;
        case 5: return pressed && position >= p[0] ? 2 : 1;
        case 6: return pressed && p[0] != 0 && p[1 + position] != 0 ? 7 : 6;
        default: return 0;
    }
}

#endif
