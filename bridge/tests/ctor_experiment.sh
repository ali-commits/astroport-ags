#!/bin/bash
# Constructor experiment of design.md section 3.5 / risk 1 (M0). Not part of `cargo test`.
# A stub library's ELF constructor spawns a thread that loads libvulkan.so.1 and creates a Vulkan
# instance (which loads the GPU driver). Cases: the constructor waits for the thread or not, the
# stub is a DT_NEEDED of the program or loaded later with dlopen, and with an LD_AUDIT library.
# usage: bridge/tests/ctor_experiment.sh [work dir]
set -eu
dir=${1:-$(mktemp -d)}
mkdir -p "$dir"
cd "$dir"

cat > stub.c <<'C'
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

typedef struct { int sType; const void* pNext; unsigned flags; const void* app; unsigned layers;
                 const char* const* layerNames; unsigned exts; const char* const* extNames; } InstanceInfo;

static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static void* load(void* unused) {
    double start = now();
    void* lib = dlopen("libvulkan.so.1", RTLD_NOW);
    if (!lib) { printf("thread: dlopen failed: %s\n", dlerror()); return NULL; }
    int (*create)(const InstanceInfo*, const void*, void**) = (int (*)(const InstanceInfo*, const void*, void**))dlsym(lib, "vkCreateInstance");
    int (*devices)(void*, unsigned*, void*) = (int (*)(void*, unsigned*, void*))dlsym(lib, "vkEnumeratePhysicalDevices");
    InstanceInfo info = { .sType = 1 };
    void* instance = NULL;
    unsigned count = 0;
    int result = create(&info, NULL, &instance);
    if (result == 0) devices(instance, &count, NULL);
    printf("thread: vulkan loaded, vkCreateInstance=%d, %u device(s), %.0f ms\n", result, count, (now() - start) * 1e3);
    fflush(stdout);
    return unused;
}

__attribute__((constructor)) static void ctor(void) {
    pthread_t thread;
    pthread_create(&thread, NULL, load, NULL);
    if (getenv("CTOR_WAIT")) {
        struct timespec limit;
        clock_gettime(CLOCK_REALTIME, &limit);
        limit.tv_sec += 10;
        int r = pthread_timedjoin_np(thread, NULL, &limit);
        printf("constructor: %s\n", r ? "TIMED OUT after 10 s waiting for the thread (deadlock)" : "thread finished while the constructor waited");
    } else {
        printf("constructor: returned without waiting\n");
    }
    fflush(stdout);
}
C
cat > needed.c <<'C'
#include <stdio.h>
#include <unistd.h>
int main(void) { puts("main: reached"); fflush(stdout); sleep(3); return 0; }
C
cat > opened.c <<'C'
#include <dlfcn.h>
#include <stdio.h>
#include <unistd.h>
int main(void) { if (!dlopen("./libstub.so", RTLD_NOW)) return 1; puts("main: reached"); fflush(stdout); sleep(3); return 0; }
C
cc -shared -fPIC -o libstub.so stub.c
cc -o needed needed.c -Wl,--no-as-needed -L. -lstub -Wl,-rpath,'$ORIGIN'
cc -o opened opened.c

cat > audit.c <<'C'
unsigned la_version(unsigned version) { return version; }
C
cc -shared -fPIC -o libaudit.so audit.c

# program, CTOR_WAIT, LD_AUDIT (the product starts the game with an audit library)
for case in "needed 1" "needed" "needed 1 ./libaudit.so" "opened 1" "opened"; do
    read -r program wait audit <<< "$case"
    echo "== $program, constructor $([ -n "$wait" ] && echo waits || echo does not wait)${audit:+, LD_AUDIT}"
    env ${wait:+CTOR_WAIT=1} ${audit:+LD_AUDIT=$audit} timeout 30 "./$program" || echo "exit code $?"
done
echo "glibc: $(getconf GNU_LIBC_VERSION)"
