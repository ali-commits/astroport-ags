#ifndef GRAPHICS_HOST_GPU_IMAGEMEMORYPRESSURE_H_
#define GRAPHICS_HOST_GPU_IMAGEMEMORYPRESSURE_H_

#include <algorithm>
#include <atomic>
#include <cstdint>

// What an image allocation does when video memory is full (GraphicContext::CreateImage and
// BindSparseImageLevels, vma.cpp). An image in system memory is much slower to sample and stays
// there, so before one is placed there:
//   1. the strict allocation (video memory required);
//   2. retained objects are released and the strict allocation is tried again;
//   3. the bytes wanted are noted for the texture cache, whose next garbage collection frees
//      clean images unused for a few frames (TextureCache::RunGarbageCollector);
//   4. with the fallback on, the relaxed allocation (video memory preferred, any type allowed).
// Step 3 only asks: the cache destroys an image when the GPU has finished the work that used it,
// so nothing it could do here would free a byte before step 4, and it is called from inside the
// cache's own image creation, under the cache's lock. The image that failed still goes to system
// memory; the ones after it find room.
//
// Cooldown: in a burst of texture creation with video memory full, every image would repeat the
// failing steps 1 and 2. For 250 ms after step 4, requests at least as large as the smallest one
// that failed go straight to the relaxed allocation; smaller ones may still fit and try strictly.
// The state is racy on purpose: the worst case is one strict or one relaxed attempt too many.
namespace Libs::Graphics::ImageMemoryPressure {

constexpr uint64_t COOLDOWN_MS  = 250;
constexpr uint64_t MIN_WANTED   = uint64_t {64} << 20u; // the cache is asked for at least this
constexpr uint64_t EVICT_FRAMES = 4;                    // VramBudget's age above its critical mark

struct State {
	std::atomic<uint64_t> deadline_ms {0}; // the cooldown runs while now < deadline
	std::atomic<uint64_t> min_size {0};
	std::atomic<uint64_t> wanted {0}; // bytes the texture cache is asked to free

	[[nodiscard]] bool CoolingDown(uint64_t now_ms, uint64_t size) const noexcept {
		return now_ms < deadline_ms.load(std::memory_order_relaxed) && size >= min_size.load(std::memory_order_relaxed);
	}
	void StartCooldown(uint64_t now_ms, uint64_t size) noexcept {
		const bool running = now_ms < deadline_ms.load(std::memory_order_relaxed);
		min_size.store(running ? std::min(size, min_size.load(std::memory_order_relaxed)) : size,
		               std::memory_order_relaxed);
		deadline_ms.store(now_ms + COOLDOWN_MS, std::memory_order_relaxed);
	}
};

inline State g_state;

// allocate(bool strict) returns a Result; release() frees retained objects and returns whether a
// retry is worth it. `size` is the request's bytes (an estimate will do). *spilled: step 4 ran.
template <class Result, class Allocate, class Release>
Result Run(State& state, uint64_t now_ms, uint64_t size, bool fallback, Result out_of_memory, Allocate&& allocate,
           Release&& release, bool* spilled) {
	*spilled = false;
	if (fallback && state.CoolingDown(now_ms, size)) {
		// Out of memory even relaxed: the steps below, which release retained objects first.
		if (const auto result = allocate(false); result != out_of_memory) {
			return result;
		}
	}
	auto result = allocate(true);
	if (result == out_of_memory && release()) {
		result = allocate(true);
	}
	if (result == out_of_memory) {
		state.wanted.fetch_add(std::max(size, MIN_WANTED), std::memory_order_relaxed);
		if (fallback) {
			state.StartCooldown(now_ms, size);
			result   = allocate(false);
			*spilled = true;
		}
	}
	return result;
}

} // namespace Libs::Graphics::ImageMemoryPressure

#endif /* GRAPHICS_HOST_GPU_IMAGEMEMORYPRESSURE_H_ */
