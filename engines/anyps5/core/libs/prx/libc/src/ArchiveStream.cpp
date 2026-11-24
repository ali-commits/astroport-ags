#include "prx/libc/include/ArchiveStream.hpp"
#include "prx/libc/include/AppArchive.hpp"
#include "prx/libc/include/General.hpp"
#include "prx/libc/include/GuestArena.hpp"

#include <cctype>
#include <cerrno>
#include <climits>
#include <clocale>
#include <cstring>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>

namespace ArchiveStream {

namespace {

constexpr std::size_t NumericFieldLimit = 4096;

int Descriptor(std::FILE* file) {
#ifdef _WIN32
    return ::_fileno(file);
#else
    return ::fileno(file);
#endif
}

int Fail(FileStream* stream, int error) {
    errno = error;
    stream->SetError();
    return -1;
}

[[noreturn]] void RefuseScan(const std::string& what) {
    NotImplemented_nid_no_patch(("fscanf " + what + " on an app0.zar stream").c_str());
}

bool IsPrefix(std::string_view text, std::string_view word) {
    if (text.size() > word.size()) return false;
    for (std::size_t i = 0; i < text.size(); ++i) if ((text[i] | 0x20) != word[i]) return false;
    return true;
}

bool IsFloating(char conversion) {
    return std::strchr("aAeEfFgG", conversion) != nullptr;
}

bool NumberPrefix(char conversion, std::string_view text) {
    if (conversion == 'p' && text.size() <= 5 && text == std::string_view("(nil)").substr(0, text.size())) return true;
    if (!text.empty() && (text[0] == '+' || text[0] == '-')) text.remove_prefix(1);
    const bool floating = IsFloating(conversion);
    if (floating && (IsPrefix(text, "infinity") || IsPrefix(text, "nan"))) return true;
    bool hex = conversion == 'x' || conversion == 'X' || conversion == 'p';
    if (text.size() >= 2 && text[0] == '0' && (text[1] | 0x20) == 'x') {
        if (!hex && !floating && conversion != 'i') return false;
        hex = true;
        text.remove_prefix(2);
    }
    const int base = hex ? 16 : conversion == 'o' || (conversion == 'i' && !text.empty() && text[0] == '0') ? 8 : 10;
    const auto digit = [base](char value) {
        return base == 16 ? std::isxdigit(static_cast<unsigned char>(value)) != 0 : value >= '0' && value < '0' + base;
    };
    std::size_t index = 0;
    std::size_t digits = 0;
    for (; index < text.size() && digit(text[index]); ++index) ++digits;
    if (!floating) return index == text.size();
    if (index < text.size() && text[index] == '.')
        for (++index; index < text.size() && digit(text[index]); ++index) ++digits;
    if (index == text.size()) return true;
    if (digits == 0 || (text[index] | 0x20) != (hex ? 'p' : 'e')) return false;
    ++index;
    if (index < text.size() && (text[index] == '+' || text[index] == '-')) ++index;
    while (index < text.size() && text[index] >= '0' && text[index] <= '9') ++index;
    return index == text.size();
}

bool InSet(std::string_view set, unsigned char value) {
    const bool negate = !set.empty() && set[0] == '^';
    if (negate) set.remove_prefix(1);
    bool found = false;
    for (std::size_t index = 0; index < set.size(); ++index) {
        const auto low = static_cast<unsigned char>(set[index]);
        if (index + 2 < set.size() && set[index + 1] == '-') {
            const auto high = static_cast<unsigned char>(set[index + 2]);
            if (low > high) RefuseScan("scanset with a descending range");
            if (value >= low && value <= high) found = true;
            index += 2;
        } else if (value == low) found = true;
    }
    return found != negate;
}

std::size_t NumberSize(const std::string& length, char conversion) {
    if (IsFloating(conversion)) return length.empty() ? sizeof(float) : length == "l" ? sizeof(double) : sizeof(long double);
    if (conversion == 'p') return sizeof(void*);
    return length == "hh" ? 1 : length == "h" ? 2 : length.empty() ? 4 : 8;
}

}

Lock::Lock(FileStream* stream, bool locked) : file(locked ? stream->GetHandle() : nullptr) {
    if (file == nullptr) return;
#ifdef _WIN32
    ::_lock_file(file);
#else
    ::flockfile(file);
#endif
}

Lock::~Lock() {
    if (file == nullptr) return;
#ifdef _WIN32
    ::_unlock_file(file);
#else
    ::funlockfile(file);
#endif
}

bool Is(FileStream* stream, bool locked) {
    if (stream == nullptr || !stream->IsOpen()) return false;
    const Lock lock(stream, locked);
    const int descriptor = Descriptor(stream->GetHandle());
    const bool archived = AppArchiveDescriptor_nid_no_patch(descriptor, nullptr, nullptr);
    if (archived ? stream->Archive().descriptor != descriptor : stream->IsArchiveCached()) stream->SetArchive(archived ? descriptor : -1);
    return archived;
}

std::size_t Read(FileStream* stream, void* buffer, std::size_t bytes, bool locked) {
    const Lock lock(stream, locked);
    auto& state = stream->Archive();
    auto* output = static_cast<unsigned char*>(buffer);
    std::size_t done = 0;
    if (bytes != 0 && state.pushed >= 0) {
        output[done++] = static_cast<unsigned char>(state.pushed);
        state.pushed = -1;
    }
    if (done < bytes) {
        const AppArchiveBuffer part{output + done, bytes - done};
        std::uint64_t read = 0;
        try {
            if (!AppArchiveDescriptorRead_nid_no_patch(state.descriptor, &part, 1, nullptr, &read)) {
                Fail(stream, 9);
                return done;
            }
        } catch (const std::runtime_error&) {
            Fail(stream, 5);
            return done;
        }
        done += read;
        if (done < bytes) state.end = true;
    }
    stream->SyncStatus();
    return done;
}

std::size_t ReadElements(FileStream* stream, void* buffer, std::size_t size, std::size_t count) {
    if (size != 0 && count > std::numeric_limits<std::size_t>::max() / size) {
        Fail(stream, 22);
        return 0;
    }
    if (size == 0 || count == 0) return 0;
    if (buffer == nullptr) throw std::runtime_error("fread: null buffer");
    const GuestArena::HostWrite destination(buffer, size * count);
    if (!destination.Open()) throw std::runtime_error("fread: buffer is not writable");
    return Read(stream, buffer, size * count) / size;
}

int Get(FileStream* stream, bool locked) {
    unsigned char value = 0;
    return Read(stream, &value, 1, locked) == 1 ? value : EOF;
}

int Peek(FileStream* stream) {
    auto& state = stream->Archive();
    if (state.pushed < 0) {
        const int value = Get(stream);
        if (value == EOF) return EOF;
        state.pushed = value;
        state.peeked = true;
    }
    return state.pushed;
}

int Unget(FileStream* stream, int value) {
    const Lock lock(stream);
    auto& state = stream->Archive();
    if (value == EOF || state.pushed >= 0) return EOF;
    state.pushed = static_cast<unsigned char>(value);
    state.peeked = false;
    state.end = false;
    stream->SyncStatus();
    return state.pushed;
}

char* Gets(FileStream* stream, char* buffer, int size) {
    const Lock lock(stream);
    const GuestArena::HostWrite destination(buffer, static_cast<std::size_t>(size));
    if (!destination.Open()) throw std::runtime_error("fgets: buffer is not writable");
    int count = 0;
    bool failed = false;
    while (count < size - 1) {
        const int value = Get(stream);
        if (value == EOF) {
            failed = !stream->Archive().end;
            break;
        }
        buffer[count++] = static_cast<char>(value);
        if (value == '\n') break;
    }
    if (size > 1 && (count == 0 || failed)) return nullptr;
    buffer[count] = '\0';
    return buffer;
}

std::int64_t Tell(FileStream* stream) {
    const Lock lock(stream);
    const auto& state = stream->Archive();
    std::uint64_t position = 0;
    if (!AppArchiveDescriptor_nid_no_patch(state.descriptor, nullptr, &position)) return Fail(stream, 9);
    if (position > static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())) {
        errno = 84;
        return -1;
    }
    if (state.pushed >= 0 && position == 0) NotImplemented_nid_no_patch("ftell on an app0.zar stream with a byte pushed back at position 0");
    return static_cast<std::int64_t>(position) - (state.pushed >= 0 ? 1 : 0);
}

int Seek(FileStream* stream, std::int64_t offset, int origin) {
    const Lock lock(stream);
    auto& state = stream->Archive();
    if (origin == SEEK_CUR && state.pushed >= 0) {
        if (offset == std::numeric_limits<std::int64_t>::min()) {
            errno = 22;
            return -1;
        }
        --offset;
    }
    std::int64_t position = 0;
    if (!AppArchiveDescriptorSeek_nid_no_patch(state.descriptor, offset, origin, &position)) return Fail(stream, 9);
    if (position < 0) {
        errno = 22;
        return -1;
    }
    state.pushed = -1;
    state.end = false;
    stream->SyncStatus();
    return 0;
}

int Flush(FileStream* stream) {
    const Lock lock(stream);
    auto& state = stream->Archive();
    if (state.pushed < 0) return 0;
    const bool end = state.end;
    if (Seek(stream, 0, SEEK_CUR) != 0) NotImplemented_nid_no_patch("fflush of an app0.zar stream with a byte pushed back at position 0");
    state.end = end;
    stream->SyncStatus();
    return 0;
}

int Purge(FileStream* stream) {
    const Lock lock(stream);
    stream->Archive().pushed = -1;
    return 0;
}

int RefuseWrite(FileStream* stream, bool locked) {
    const Lock lock(stream, locked);
    return Fail(stream, 9);
}

int SetBuffering(int mode) {
    if (mode < 0 || mode > 2) {
        errno = 22;
        return -1;
    }
    if (mode != 2) NotImplemented_nid_no_patch("setvbuf with full or line buffering on an app0.zar stream");
    return 0;
}

void RequireScanFormat(const char* format) {
    for (const char* cursor = format; *cursor != '\0';) {
        if (*cursor++ != '%') continue;
        if (*cursor == '%') {
            ++cursor;
            continue;
        }
        if (*cursor == '*') ++cursor;
        if (std::isdigit(static_cast<unsigned char>(*cursor))) {
            unsigned long long width = 0;
            for (; std::isdigit(static_cast<unsigned char>(*cursor)); ++cursor)
                if (width <= INT_MAX) width = width * 10 + static_cast<unsigned long long>(*cursor - '0');
            if (*cursor == '$') RefuseScan("positional argument");
            if (width == 0 || width > INT_MAX) RefuseScan("field width outside 1..INT_MAX");
        }
        if (*cursor == 'm') RefuseScan("allocating conversion");
        std::string length;
        if ((cursor[0] == 'h' && cursor[1] == 'h') || (cursor[0] == 'l' && cursor[1] == 'l')) length.assign(cursor, 2);
        else if (*cursor != '\0' && std::strchr("hljztL", *cursor) != nullptr) length.assign(cursor, 1);
        cursor += length.size();
        const char conversion = *cursor;
        if (conversion != '\0' && std::strchr("diouxXn", conversion) != nullptr) {
            if (length == "L") RefuseScan("integer conversion with length L");
        } else if (conversion == 'p') {
            if (!length.empty()) RefuseScan("pointer conversion with a length modifier");
        } else if (conversion != '\0' && IsFloating(conversion)) {
            if (!length.empty() && length != "l" && length != "L") RefuseScan("floating-point conversion with length " + length);
            if (std::strcmp(std::localeconv()->decimal_point, ".") != 0) RefuseScan("floating-point conversion with a decimal point other than '.'");
#ifdef _WIN32
            if (length == "L") RefuseScan("long double conversion on Windows");
#endif
        } else if (conversion == 's' || conversion == 'c' || conversion == '[') {
            if (length == "l") RefuseScan("wide conversion");
            if (!length.empty()) RefuseScan("text conversion with length " + length);
            if (conversion == '[') {
                ++cursor;
                if (*cursor == '^') ++cursor;
                if (*cursor == ']') ++cursor;
                while (*cursor != '\0' && *cursor != ']') ++cursor;
                if (*cursor != ']') RefuseScan("unterminated scanset");
            }
        } else if (conversion == 'S' || conversion == 'C') {
            RefuseScan("wide conversion");
        } else {
            RefuseScan("unknown conversion");
        }
        ++cursor;
    }
}

void EndScan(FileStream* stream) {
    const auto& state = stream->Archive();
    if (state.pushed >= 0 && state.peeked) Flush(stream);
}

void Source::Store(void* target, const void* source, std::size_t bytes) {
    const GuestArena::HostWrite destination(target, bytes);
    if (!destination.Open()) throw std::runtime_error("fscanf: destination is not writable");
    std::memcpy(target, source, bytes);
}

int Source::Scan(const char* directive, bool suppress, void* target, int* count) const {
    std::string_view rest(directive + (suppress ? 2 : 1));
    std::size_t width = 0;
    for (; std::isdigit(static_cast<unsigned char>(rest.front())); rest.remove_prefix(1)) width = width * 10 + static_cast<std::size_t>(rest.front() - '0');
    rest.remove_suffix(2);
    const std::size_t lengthSize = rest.find_first_not_of("hljztL");
    const std::string length(rest.substr(0, lengthSize));
    const char conversion = rest[lengthSize];
    std::string text;
    if (conversion == 's' || conversion == 'c' || conversion == '[') {
        const auto set = conversion == '[' ? rest.substr(lengthSize + 1, rest.size() - lengthSize - 2) : std::string_view();
        const std::size_t limit = width != 0 ? width : conversion == 'c' ? 1 : std::numeric_limits<std::size_t>::max();
        while (text.size() < limit) {
            const int next = ArchiveStream::Peek(stream);
            if (next == EOF || (conversion == 's' && std::isspace(next)) || (conversion == '[' && !InSet(set, static_cast<unsigned char>(next)))) break;
            Get(stream);
            text.push_back(static_cast<char>(next));
        }
        if (text.empty()) return 0;
        *count = static_cast<int>(text.size());
        if (!suppress) Store(target, text.c_str(), text.size() + (conversion == 'c' ? 0 : 1));
        return suppress ? 0 : 1;
    }
    const std::size_t limit = width != 0 ? width : std::numeric_limits<std::size_t>::max();
    while (text.size() < limit) {
        const int next = ArchiveStream::Peek(stream);
        if (next == EOF || !NumberPrefix(conversion, text + static_cast<char>(next))) break;
        if (text.size() == NumericFieldLimit) RefuseScan("numeric field longer than 4096 bytes");
        Get(stream);
        text.push_back(static_cast<char>(next));
    }
    int parsed = -1;
    std::sscanf(text.c_str(), ("%*" + length + conversion + "%n").c_str(), &parsed);
    if (text.empty()) return 0;
    if (parsed != static_cast<int>(text.size())) {
        *count = -2;
        return 0;
    }
    if (!suppress) {
        alignas(16) unsigned char value[16]{};
        if (std::sscanf(text.c_str(), ("%" + length + conversion).c_str(), value) != 1) {
            *count = -2;
            return 0;
        }
        Store(target, value, NumberSize(length, conversion));
    }
    *count = static_cast<int>(text.size());
    return suppress ? 0 : 1;
}

}
