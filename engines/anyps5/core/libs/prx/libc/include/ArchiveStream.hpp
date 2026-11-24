#ifndef CORE_LIBS_PRX_LIBC_INCLUDE_ARCHIVESTREAM_HPP
#define CORE_LIBS_PRX_LIBC_INCLUDE_ARCHIVESTREAM_HPP

#include "prx/libc/include/FileStream.hpp"

#include <cstddef>
#include <cstdint>
#include <cstdio>

namespace ArchiveStream {

class Lock {
    std::FILE* const file;

public:
    explicit Lock(FileStream* stream, bool locked = true);
    ~Lock();
    Lock(const Lock&) = delete;
    Lock& operator=(const Lock&) = delete;
};

bool Is(FileStream* stream, bool locked = true);
std::size_t Read(FileStream* stream, void* buffer, std::size_t bytes, bool locked = true);
std::size_t ReadElements(FileStream* stream, void* buffer, std::size_t size, std::size_t count);
int Get(FileStream* stream, bool locked = true);
int Peek(FileStream* stream);
int Unget(FileStream* stream, int value);
char* Gets(FileStream* stream, char* buffer, int size);
std::int64_t Tell(FileStream* stream);
int Seek(FileStream* stream, std::int64_t offset, int origin);
int Flush(FileStream* stream);
int Purge(FileStream* stream);
int RefuseWrite(FileStream* stream, bool locked = true);
int SetBuffering(int mode);
void RequireScanFormat(const char* format);
void EndScan(FileStream* stream);

class Source {
    FileStream* const stream;
    std::ptrdiff_t consumed = 0;

public:
    static constexpr bool Contiguous = false;

    explicit Source(FileStream* value) : stream(value) {}

    int Peek() const { return ArchiveStream::Peek(stream); }
    void Skip() {
        Get(stream);
        ++consumed;
    }
    void Advance(std::ptrdiff_t count) { consumed += count; }
    std::ptrdiff_t Consumed() const { return consumed; }
    int Scan(const char* directive, bool suppress, void* target, int* count) const;
    static void Store(void* target, const void* source, std::size_t bytes);
};

}

#endif
