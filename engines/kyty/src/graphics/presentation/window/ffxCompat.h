#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXCOMPAT_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXCOMPAT_H_

// Force-included into the FidelityFX SDK's own sources (3rdparty/CMakeLists.txt, kyty_ffx), and
// into nothing of Kyty's: the MSVC names those sources use outside their _WIN32 blocks. The copies
// truncate and always terminate.

#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <locale>

#define _countof(array) (sizeof(array) / sizeof((array)[0]))
#ifndef FFX_UNUSED
#define FFX_UNUSED(parameter) (void)(parameter)
#endif

inline int wcscpy_s(wchar_t* destination, size_t size, const wchar_t* source) {
	if (size == 0) {
		return 1;
	}
	size_t i = 0;
	for (; i + 1 < size && source[i] != 0; i++) {
		destination[i] = source[i];
	}
	destination[i] = 0;
	return 0;
}
template <size_t Size>
int wcscpy_s(wchar_t (&destination)[Size], const wchar_t* source) {
	return wcscpy_s(destination, Size, source);
}

inline int strcpy_s(char* destination, size_t size, const char* source) {
	if (size == 0) {
		return 1;
	}
	size_t i = 0;
	for (; i + 1 < size && source[i] != 0; i++) {
		destination[i] = source[i];
	}
	destination[i] = 0;
	return 0;
}
template <size_t Size>
int strcpy_s(char (&destination)[Size], const char* source) {
	return strcpy_s(destination, Size, source);
}

// snprintf truncates and terminates for any size above zero.
template <class... Args>
int sprintf_s(char* destination, size_t size, const char* format, Args... args) {
	return std::snprintf(destination, size, format, args...);
}
template <size_t Size, class... Args>
int sprintf_s(char (&destination)[Size], const char* format, Args... args) {
	return std::snprintf(destination, Size, format, args...);
}

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXCOMPAT_H_
