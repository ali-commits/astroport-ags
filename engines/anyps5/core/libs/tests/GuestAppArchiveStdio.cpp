#include "prx/libkernel/include/Api.hpp"
#include "prx/libc/include/Api.hpp"
#include "AppArchiveFixture.hpp"
#include "SceTypes.hpp"
#include "prx/libc/include/AppArchive.hpp"
#include "prx/libc/include/FileStream.hpp"
#include "prx/libc/include/general/VabiMacros.hpp"
#include "prx/libkernel/File/include/FileFlags.hpp"
#include "prx/libkernel/File/include/File.hpp"

#include <array>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <map>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace {

void Require(bool value, int line) {
    if (!value) {
        std::fprintf(stderr, "app0.zar stdio check failed at line %d (errno %d)\n", line, errno);
        std::exit(1);
    }
}
#define Check(value) Require((value), __LINE__)

template <typename TCall> bool NotImplemented(TCall call) {
    try {
        call();
    } catch (const std::runtime_error& error) {
        return std::strstr(error.what(), "not implemented") != nullptr;
    }
    return false;
}

std::vector<char> Bytes(const std::string& text) {
    return {text.begin(), text.end()};
}

constexpr std::size_t ThreadedSize = 20000;
const std::string Numbers = "42 -17 0x1F 3.5 word [abc] 100eX 1234 0x10 7 8 abcdef 55%";

FileStream* Open(const char* path) {
    auto* stream = fopen_nid_postfix(path, "rb");
    Check(stream != nullptr);
    return stream;
}

int Flags(FileStream* stream) {
    return stream->GuestState().flags;
}

std::string ReadAll(FileStream* stream) {
    std::string text;
    for (int value = fgetc_nid_postfix(stream); value != EOF; value = fgetc_nid_postfix(stream)) text.push_back(static_cast<char>(value));
    return text;
}

void Prefix() {
    auto* stream = Open("/app0/Data/Eleven.bin");
    const auto& guest = stream->GuestState();
    Check(guest.flags == 4 && guest.position == nullptr && guest.readRemaining == 0 && guest.writeRemaining == 0);
    Check(guest.buffer == nullptr && guest.bufferSize == 0);
    const int descriptor = fileno_nid_postfix(stream);
    Check(descriptor >= 0 && guest.descriptor == descriptor && fileno_unlocked_nid_postfix(stream) == descriptor);
    Check(AppArchiveDescriptor_nid_no_patch(descriptor, nullptr, nullptr));
    Check(fgetc_nid_postfix(stream) == '0' && fseek_nid_postfix(stream, 0, SEEK_END) == 0 && fgetc_nid_postfix(stream) == EOF);
    Check(guest.flags == (4 | 0x20) && guest.position == nullptr && guest.readRemaining == 0 && guest.buffer == nullptr);
    const auto* raw = reinterpret_cast<const unsigned char*>(stream);
    for (std::size_t index = sizeof(GuestFilePrefix); index < 256; ++index) Check(raw[index] == 0);
    bool refused = false;
    try { (void)GetNativeStream(stream); } catch (const std::runtime_error& error) { refused = std::strstr(error.what(), "app0.zar stream") != nullptr; }
    Check(refused);
    Check(fclose_nid_postfix(stream) == 0 && !AppArchiveDescriptor_nid_no_patch(descriptor, nullptr, nullptr));
}

void Reads() {
    auto* stream = Open("/app0/Data/Eleven.bin");
    char buffer[32] = {};
    Check(ftell_nid_postfix(stream) == 0 && !feof_nid_postfix(stream));
    Check(fread_nid_postfix(buffer, 2, 4, stream) == 4 && std::memcmp(buffer, "01234567", 8) == 0 && !feof_nid_postfix(stream));
    Check(fread_nid_postfix(buffer, 2, 4, stream) == 1 && std::memcmp(buffer, "89A", 3) == 0 && feof_nid_postfix(stream));
    Check(ftell_nid_postfix(stream) == 11 && ftello_nid_postfix(stream) == 11);
    clearerr_nid_postfix(stream);
    Check(!feof_nid_postfix(stream) && fread_nid_postfix(buffer, 0, 4, stream) == 0 && fread_nid_postfix(buffer, 4, 0, stream) == 0 && Flags(stream) == 4);
    errno = 0;
    Check(fread_nid_postfix(buffer, std::numeric_limits<std::size_t>::max(), 2, stream) == 0 && errno == 22 && ferror_nid_postfix(stream));
    Check(ftell_nid_postfix(stream) == 11 && !feof_nid_postfix(stream));
    rewind_nid_postfix(stream);
    Check(!ferror_nid_postfix(stream) && ftell_nid_postfix(stream) == 0);
    Check(fread_nid_postfix(buffer, 1, 11, stream) == 11 && !feof_nid_postfix(stream) && !feof_unlocked_nid_postfix(stream));
    Check(fgetc_nid_postfix(stream) == EOF && feof_nid_postfix(stream) && feof_unlocked_nid_postfix(stream) && !ferror_unlocked_nid_postfix(stream));
    Check(fflush_nid_postfix(stream) == 0 && feof_nid_postfix(stream));

    Check(fseek_nid_postfix(stream, 3, SEEK_SET) == 0 && !feof_nid_postfix(stream) && getc_nid_postfix(stream) == '3');
    Check(fseeko_nid_postfix(stream, -1, SEEK_CUR) == 0 && getc_unlocked_nid_postfix(stream) == '3' && __srget_nid_postfix(stream) == '4');
    Check(fseek_nid_postfix(stream, -2, SEEK_END) == 0 && fgetc_nid_postfix(stream) == '9');
    std::int64_t position = 0;
    Check(fgetpos_nid_postfix(stream, &position) == 0 && position == 10);
    Check(fseek_nid_postfix(stream, 100, SEEK_SET) == 0 && ftell_nid_postfix(stream) == 100 && fgetc_nid_postfix(stream) == EOF && feof_nid_postfix(stream));
    errno = 0;
    Check(fseek_nid_postfix(stream, -1, SEEK_SET) == -1 && errno == 22 && ftell_nid_postfix(stream) == 100);
    errno = 0;
    Check(fseek_nid_postfix(stream, -200, SEEK_CUR) == -1 && errno == 22 && ftell_nid_postfix(stream) == 100);
    errno = 0;
    Check(fseek_nid_postfix(stream, std::numeric_limits<std::int64_t>::max(), SEEK_CUR) == -1 && errno == 22 && ftell_nid_postfix(stream) == 100);
    errno = 0;
    Check(fseek_nid_postfix(stream, 0, 7) == -1 && errno == 22);
    Check(fsetpos_nid_postfix(stream, &position) == 0 && fgetc_nid_postfix(stream) == 'A');
    Check(fclose_nid_postfix(stream) == 0);

    stream = Open("/app0/Data/Empty.bin");
    Check(!feof_nid_postfix(stream) && fgetc_nid_postfix(stream) == EOF && feof_nid_postfix(stream) && !ferror_nid_postfix(stream));
    Check(fclose_nid_postfix(stream) == 0);
}

void PushBack() {
    auto* stream = Open("/app0/Data/Eleven.bin");
    Check(fseek_nid_postfix(stream, 5, SEEK_SET) == 0 && fgetc_nid_postfix(stream) == '5');
    Check(ungetc_nid_postfix('x', stream) == 'x' && ftell_nid_postfix(stream) == 5);
    Check(ungetc_nid_postfix('y', stream) == EOF && ungetc_nid_postfix(EOF, stream) == EOF);
    Check(fgetc_nid_postfix(stream) == 'x' && fgetc_nid_postfix(stream) == '6' && ftell_nid_postfix(stream) == 7);
    Check(ungetwc_nid_postfix('w', stream) == 'w' && ungetwc_nid_postfix('v', stream) == -1 && fgetc_nid_postfix(stream) == 'w');
    Check(ungetc_nid_postfix('p', stream) == 'p' && fseek_nid_postfix(stream, 0, SEEK_CUR) == 0 && ftell_nid_postfix(stream) == 6 && fgetc_nid_postfix(stream) == '6');
    errno = 0;
    Check(ungetc_nid_postfix('p', stream) == 'p' && fseek_nid_postfix(stream, std::numeric_limits<std::int64_t>::min(), SEEK_CUR) == -1 && errno == 22);
    Check(fflush_nid_postfix(stream) == 0 && ftell_nid_postfix(stream) == 6 && fgetc_nid_postfix(stream) == '6');
    Check(ungetc_nid_postfix('q', stream) == 'q' && fpurge_nid_postfix(stream) == 0 && fgetc_nid_postfix(stream) == '7');
    char buffer[4] = {};
    Check(ungetc_nid_postfix('r', stream) == 'r' && fread_nid_postfix(buffer, 1, 3, stream) == 3 && std::memcmp(buffer, "r89", 3) == 0);
    Check(fgetc_nid_postfix(stream) == 'A' && fgetc_nid_postfix(stream) == EOF && feof_nid_postfix(stream));
    Check(ungetc_nid_postfix('z', stream) == 'z' && !feof_nid_postfix(stream) && fgetc_nid_postfix(stream) == 'z');
    Check(fclose_nid_postfix(stream) == 0);
}

void Refusals() {
    auto* stream = Open("/app0/Data/Eleven.bin");
    Check(fgetc_nid_postfix(stream) == '0');
    const auto refused = [stream](auto call, int failure) {
        clearerr_nid_postfix(stream);
        errno = 0;
        return call() == failure && errno == 9 && ferror_nid_postfix(stream) && !feof_nid_postfix(stream) && ftell_nid_postfix(stream) == 1;
    };
    Check(refused([stream] { return static_cast<int>(fwrite_nid_postfix("a", 1, 1, stream)); }, 0));
    Check(refused([stream] { return fputc_nid_postfix('a', stream); }, EOF));
    Check(refused([stream] { return putc_nid_postfix('a', stream); }, EOF));
    Check(refused([stream] { return putc_unlocked_nid_postfix('a', stream); }, EOF));
    Check(refused([stream] { return fputs_nid_postfix("a", stream); }, EOF));
    Check(refused([stream] { return fprintf_nid_postfix(stream, "%d", 1); }, -1));
    Check(refused([stream] { return fprintf_s_nid_postfix(stream, "%d", 1); }, -1));
    clearerr_nid_postfix(stream);
    Check(fwrite_nid_postfix("a", 0, 1, stream) == 0 && fwrite_nid_postfix("a", 1, 0, stream) == 0 && !ferror_nid_postfix(stream));

    Check(setvbuf_nid_postfix(stream, nullptr, 2, 0) == 0);
    setbuf_nid_postfix(stream, nullptr);
    errno = 0;
    Check(setvbuf_nid_postfix(stream, nullptr, 7, 0) == -1 && errno == 22);
    char storage[64];
    Check(NotImplemented([&] { (void)setvbuf_nid_postfix(stream, storage, 0, sizeof(storage)); }));
    Check(NotImplemented([&] { (void)setvbuf_nid_postfix(stream, storage, 1, sizeof(storage)); }));
    int value = 0;
    Check(NotImplemented([&] { (void)fwscanf_nid_postfix(stream, u"%d", &value); }));
    Check(fgetc_nid_postfix(stream) == '1' && fclose_nid_postfix(stream) == 0);

    errno = 0;
    Check(fopen_nid_postfix("/app0/Data/Eleven.bin", "wx") == nullptr && errno == 17);
    for (const char* mode : {"w", "a", "r+", "w+", "a+"}) {
        errno = 0;
        Check(fopen_nid_postfix("/app0/Data/Eleven.bin", mode) == nullptr && errno == 30);
    }
    errno = 0;
    Check(fopen_nid_postfix("/app0/Data", "r") == nullptr && errno == 21);
    errno = 0;
    Check(fopen_nid_postfix("/app0/Data/Missing.bin", "r") == nullptr && errno == 2);
}

void Lines() {
    auto* stream = Open("/app0/Text/Lines.txt");
    char buffer[64];
    Check(fgets_nid_postfix(buffer, sizeof(buffer), stream) == buffer && std::strcmp(buffer, "first\n") == 0);
    Check(fgets_nid_postfix(buffer, 4, stream) == buffer && std::strcmp(buffer, "sec") == 0);
    buffer[0] = 'x';
    Check(fgets_nid_postfix(buffer, 1, stream) == buffer && buffer[0] == '\0' && ftell_nid_postfix(stream) == 9);
    Check(fgets_nid_postfix(buffer, sizeof(buffer), stream) == buffer && std::strcmp(buffer, "ond line\n") == 0 && !feof_nid_postfix(stream));
    Check(fgets_nid_postfix(buffer, sizeof(buffer), stream) == buffer && std::strcmp(buffer, "last") == 0 && feof_nid_postfix(stream));
    Check(fgets_nid_postfix(buffer, sizeof(buffer), stream) == nullptr && !ferror_nid_postfix(stream));
    errno = 0;
    Check(fgets_nid_postfix(buffer, 0, stream) == nullptr && errno == 22 && ferror_nid_postfix(stream));
    Check(fclose_nid_postfix(stream) == 0);
}

void Priority() {
    auto* stream = Open("/app0/Text/Shadow.txt");
    Check((Flags(stream) & ~0x60) == 0x10 && ReadAll(stream) == "disk" && fclose_nid_postfix(stream) == 0);
    stream = Open("/app0/Loose.txt");
    Check(ReadAll(stream) == "loose" && fclose_nid_postfix(stream) == 0);
    stream = Open("/app0/Assets/Icon.odx");
    Check(Flags(stream) == 4 && ReadAll(stream) == "icon" && fclose_nid_postfix(stream) == 0);
}

void Scanning() {
    auto* stream = Open("/app0/Text/Numbers.txt");
    int first = 0, second = 0, count = 0;
    unsigned third = 0;
    float real = 0;
    char word[16] = {}, set[16] = {};
    Check(fscanf_nid_postfix(stream, "%d %d %x %f %s [%[a-c]]%n", &first, &second, &third, &real, word, set, &count) == 6);
    Check(first == 42 && second == -17 && third == 0x1f && real == 3.5f && std::strcmp(word, "word") == 0 && std::strcmp(set, "abc") == 0);
    const auto consumed = static_cast<std::int64_t>(Numbers.find("]") + 1);
    Check(count == consumed && ftell_nid_postfix(stream) == consumed && sceKernelLseek(fileno_nid_postfix(stream), 0, 1) == consumed);
    Check(fscanf_nid_postfix(stream, " %f", &real) == 0 && ftell_nid_postfix(stream) == consumed + 5 && real == 3.5f);
    Check(fgetc_nid_postfix(stream) == 'X');
    signed char tiny = 0;
    long wide = 0;
    void* pointer = nullptr;
    std::int64_t longCount = 0;
    Check(fscanf_nid_postfix(stream, "%2d%2d %p %hhd %*d%ln", &first, &second, &pointer, &tiny, &longCount) == 4);
    Check(first == 12 && second == 34 && pointer == reinterpret_cast<void*>(0x10) && tiny == 7 && longCount == 14);
    char letters[4] = {};
    Check(fscanf_nid_postfix(stream, " %3c%[^ ] %ld%%", letters, set, &wide) == 3 && std::memcmp(letters, "abc", 3) == 0 && std::strcmp(set, "def") == 0 && wide == 55);
    Check(ftell_nid_postfix(stream) == static_cast<std::int64_t>(Numbers.size()) && sceKernelLseek(fileno_nid_postfix(stream), 0, 1) == static_cast<std::int64_t>(Numbers.size()));
    Check(fscanf_nid_postfix(stream, "%d", &first) == EOF && feof_nid_postfix(stream) && !ferror_nid_postfix(stream));

    rewind_nid_postfix(stream);
    for (const char* format : {"%ls", "%lc", "%l[a]", "%S", "%C", "%1$d", "%ms", "%q", "%[abc", "%0d", "%99999999999d", "%Ld", "%hs", "%lp"}) {
        Check(NotImplemented([&] { (void)fscanf_nid_postfix(stream, format, word); }));
        Check(ftell_nid_postfix(stream) == 0);
    }
    Check(fseek_nid_postfix(stream, static_cast<std::int64_t>(Numbers.find("word")), SEEK_SET) == 0);
    word[0] = 'x';
    Check(fscanf_s_nid_postfix(stream, "%s", word, 3u) == 0 && word[0] == '\0');
    Check(fseek_nid_postfix(stream, static_cast<std::int64_t>(Numbers.find("word")), SEEK_SET) == 0);
    Check(fscanf_s_nid_postfix(stream, "%s %c", word, 5u, letters, 1u) == 2 && std::strcmp(word, "word") == 0 && letters[0] == '[');
    Check(fclose_nid_postfix(stream) == 0);

    stream = Open("/app0/Text/Long.txt");
    Check(NotImplemented([&] { (void)fscanf_nid_postfix(stream, "%d", &first); }));
    Check(ftell_nid_postfix(stream) == 4096 && sceKernelLseek(fileno_nid_postfix(stream), 0, 1) == 4096);
    Check(fseek_nid_postfix(stream, 0, SEEK_SET) == 0);
    std::vector<char> text(6000);
    Check(fscanf_nid_postfix(stream, "%s", text.data()) == 1 && std::strlen(text.data()) == 5000);
    Check(fclose_nid_postfix(stream) == 0);

    stream = Open("/app0/Text/Minus.txt");
    Check(fscanf_nid_postfix(stream, "%d", &first) == 0 && ftell_nid_postfix(stream) == 1);
    Check(fscanf_nid_postfix(stream, "%d", &first) == EOF);
    Check(fclose_nid_postfix(stream) == 0);
}

void SharedDescriptors() {
    const int fd = sceKernelOpen("/app0/Data/Eleven.bin", SCE_KERNEL_O_RDONLY, 0);
    char buffer[8] = {};
    Check(fd >= 0 && sceKernelRead(fd, buffer, 3) == 3);
    errno = 0;
    Check(fdopen_nid_postfix(fd, "w") == nullptr && errno == 9);
    errno = 0;
    Check(fdopen_nid_postfix(fd, "r+") == nullptr && errno == 9);
    Check(fdopen_nid_postfix(-1, "r") == nullptr);
    errno = 0;
    Check(fdopen_nid_postfix(fd, "q") == nullptr && errno == 22);
    auto* stream = fdopen_nid_postfix(fd, "r");
    Check(stream != nullptr && Flags(stream) == 4 && ftell_nid_postfix(stream) == 3 && fgetc_nid_postfix(stream) == '3');
    const int duplicate = dup_nid_postfix(fd);
    Check(duplicate >= 0 && sceKernelLseek(duplicate, 0, 1) == 4);
    Check(fread_nid_postfix(buffer, 1, 2, stream) == 2 && sceKernelLseek(duplicate, 0, 1) == 6);
    Check(sceKernelRead(duplicate, buffer, 1) == 1 && buffer[0] == '6' && fgetc_nid_postfix(stream) == '7');
    Check(fclose_nid_postfix(stream) == 0 && !AppArchiveDescriptor_nid_no_patch(fd, nullptr, nullptr));
    Check(sceKernelRead(duplicate, buffer, 1) == 1 && buffer[0] == '8' && sceKernelClose(duplicate) == 0);

    stream = Open("/app0/Loose.txt");
    Check((Flags(stream) & ~0x60) == 0x10 && fgetc_nid_postfix(stream) == 'l');
    const int archived = sceKernelOpen("/app0/Data/Eleven.bin", SCE_KERNEL_O_RDONLY, 0);
    const int loose = sceKernelOpen("/app0/Loose.txt", SCE_KERNEL_O_RDONLY, 0);
    const int target = fileno_nid_postfix(stream);
    Check(archived >= 0 && loose >= 0 && dup2_nid_postfix(archived, target) == target);
    Check(fgetc_nid_postfix(stream) == '0' && Flags(stream) == 4 && sceKernelLseek(archived, 0, 1) == 1 && fileno_nid_postfix(stream) == target);
    Check(ungetc_nid_postfix('u', stream) == 'u' && dup2_nid_postfix(loose, target) == target);
    Check(!feof_nid_postfix(stream) && (Flags(stream) & ~0x60) == 0x10 && (void*)GetNativeStream(stream) != nullptr);
    Check(dup2_nid_postfix(archived, target) == target && fgetc_nid_postfix(stream) == '1' && Flags(stream) == 4);
    Check(fclose_nid_postfix(stream) == 0 && sceKernelClose(archived) == 0 && sceKernelClose(loose) == 0);
}

void Reopening() {
    auto* stream = Open("/app0/Data/Eleven.bin");
    int previous = fileno_nid_postfix(stream);
    Check(freopen_nid_postfix("/app0/Text/Lines.txt", "r", stream) == stream && Flags(stream) == 4 && fgetc_nid_postfix(stream) == 'f');
    Check(!AppArchiveDescriptor_nid_no_patch(previous, nullptr, nullptr) || fileno_nid_postfix(stream) == previous);
    previous = fileno_nid_postfix(stream);
    Check(freopen_nid_postfix("/app0/Loose.txt", "r", stream) == stream && (Flags(stream) & ~0x60) == 0x10 && ReadAll(stream) == "loose");
    Check(!AppArchiveDescriptor_nid_no_patch(previous, nullptr, nullptr) && !AppArchiveDescriptor_nid_no_patch(fileno_nid_postfix(stream), nullptr, nullptr));
    Check(freopen_nid_postfix("/app0/Data/Eleven.bin", "rb", stream) == stream && Flags(stream) == 4 && ReadAll(stream) == "0123456789A");
    char buffer[4];
    Check(freopen_nid_postfix("/dev/urandom", "r", stream) == stream && stream->IsRandomDevice() && fread_nid_postfix(buffer, 1, 4, stream) == 4);
    Check(freopen_nid_postfix("/app0/Assets/Icon.odx", "r", stream) == stream && !stream->IsRandomDevice() && ReadAll(stream) == "icon");
    previous = fileno_nid_postfix(stream);
    errno = 0;
    Check(freopen_nid_postfix("/app0/Data/Missing.bin", "r", stream) == nullptr && errno == 2 && !AppArchiveDescriptor_nid_no_patch(previous, nullptr, nullptr));
    stream = Open("/app0/Loose.txt");
    errno = 0;
    Check(freopen_nid_postfix("/app0/Data/Eleven.bin", "w", stream) == nullptr && errno == 30);
}

void Threads() {
    auto* stream = Open("/app0/Data/Threaded.bin");
    std::array<std::atomic<int>, 256> seen{};
    std::vector<std::thread> workers;
    for (int index = 0; index < 4; ++index) workers.emplace_back([&] {
        for (int value = fgetc_nid_postfix(stream); value != EOF; value = fgetc_nid_postfix(stream)) ++seen[static_cast<std::size_t>(value)];
    });
    for (auto& worker : workers) worker.join();
    std::array<int, 256> expected{};
    for (const char value : AppArchiveFixture::Pattern(ThreadedSize)) ++expected[static_cast<unsigned char>(value)];
    for (std::size_t index = 0; index < 256; ++index) Check(seen[index] == expected[index]);
    Check(!ferror_nid_postfix(stream) && feof_nid_postfix(stream) && ftell_nid_postfix(stream) == static_cast<std::int64_t>(ThreadedSize));
    Check(fclose_nid_postfix(stream) == 0);
}

void Archive(const std::filesystem::path& root) {
    AppArchiveFixture::WriteArchive(root / "app0.zar", {
        {"Assets/Icon.odxb", Bytes("icon")},
        {"Data/Eleven.bin", Bytes("0123456789A")},
        {"Data/Empty.bin", {}},
        {"Data/Threaded.bin", AppArchiveFixture::Pattern(ThreadedSize)},
        {"Text/Lines.txt", Bytes("first\nsecond line\nlast")},
        {"Text/Long.txt", std::vector<char>(5000, '1')},
        {"Text/Minus.txt", Bytes("-")},
        {"Text/Numbers.txt", Bytes(Numbers)},
        {"Text/Shadow.txt", Bytes("zar!")},
    });
    std::filesystem::create_directories(root / "app0" / "Text");
    { std::ofstream loose(root / "app0" / "Loose.txt", std::ios::binary); loose << "loose"; }
    { std::ofstream shadow(root / "app0" / "Text" / "Shadow.txt", std::ios::binary); shadow << "disk"; }
    Prefix();
    Reads();
    PushBack();
    Refusals();
    Lines();
    Priority();
    Scanning();
    SharedDescriptors();
    Reopening();
    Threads();
}

void Absent(const std::filesystem::path& root) {
    { std::ofstream loose(root / "app0" / "Loose.txt", std::ios::binary); loose << "loose"; }
    auto* stream = Open("/app0/Loose.txt");
    Check((Flags(stream) & ~0x60) == 0x10 && ReadAll(stream) == "loose" && fclose_nid_postfix(stream) == 0);
    errno = 0;
    Check(fopen_nid_postfix("/app0/Data/Eleven.bin", "rb") == nullptr && errno == 2);
}

void Damaged(const std::filesystem::path& root) {
    AppArchiveFixture::WriteArchive(root / "app0.zar", {{"Numbers.txt", Bytes(Numbers)}});
    {
        std::fstream archive(root / "app0.zar", std::ios::binary | std::ios::in | std::ios::out);
        const char garbage[8] = {0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x00};
        archive.seekp(0);
        archive.write(garbage, sizeof(garbage));
    }
    auto* stream = Open("/app0/Numbers.txt");
    const auto failed = [stream] {
        const bool result = errno == 5 && ferror_nid_postfix(stream) && !feof_nid_postfix(stream) && ftell_nid_postfix(stream) == 0;
        clearerr_nid_postfix(stream);
        errno = 0;
        return result;
    };
    char buffer[8];
    int value = 0;
    errno = 0;
    Check(fread_nid_postfix(buffer, 1, sizeof(buffer), stream) == 0 && failed());
    Check(fgetc_nid_postfix(stream) == EOF && failed());
    Check(fgets_nid_postfix(buffer, sizeof(buffer), stream) == nullptr && failed());
    Check(fscanf_nid_postfix(stream, "%d", &value) == EOF && failed());
    Check(fclose_nid_postfix(stream) == 0);
}

}

int main(int argc, char** argv) {
    if (argc != 2) {
        std::fprintf(stderr, "usage: guest_app_archive_stdio_tests archive|absent|damaged\n");
        return 2;
    }
    const std::string mode = argv[1];
    const auto root = AppArchiveFixture::EnterFreshRoot("anyps5-app-archive-stdio");
    if (mode == "archive") Archive(root);
    else if (mode == "absent") Absent(root);
    else if (mode == "damaged") Damaged(root);
    else return 2;
    std::error_code error;
    std::filesystem::current_path(std::filesystem::temp_directory_path(), error);
    std::filesystem::remove_all(root, error);
    return 0;
}
