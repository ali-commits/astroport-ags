#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGENLOGIC_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGENLOGIC_H_

// The decisions of FSR frame generation (frameGen.h) that need neither Vulkan nor the SDK:
// pacing, the display gate, frame continuity and the capture ring's bookkeeping.
// Tested in tests/FrameGenTests.cpp.

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <string>

namespace Libs::Graphics::FrameGen {

// Captures kept: a presented frame names its capture by serial, and the render thread may be up
// to kSlots - 1 captures ahead of the present thread before that one reads as lost.
constexpr uint32_t kSlots = 4;

[[nodiscard]] constexpr uint32_t SlotOf(uint64_t serial) {
	return static_cast<uint32_t>(serial % kSlots);
}

// The interval between real frames, for showing the generated one half an interval earlier.
class Pacer {
public:
	// A new real frame at `now_ns`.
	void Note(uint64_t now_ns) {
		if (m_last != 0 && now_ns > m_last && now_ns - m_last <= kGapNs) {
			m_intervals[m_next] = now_ns - m_last;
			m_next              = (m_next + 1) % m_intervals.size();
			m_count             = std::min(m_count + 1, m_intervals.size());
		} else {
			m_count = 0; // the first frame, or frames stopped for a while: measure again
			m_next  = 0;
		}
		m_last = now_ns;
	}
	// Half the average interval, 2..50 ms; 0 while fewer than two intervals are known.
	[[nodiscard]] uint64_t HalfNs() const {
		if (m_count < 2) {
			return 0;
		}
		uint64_t sum = 0;
		for (size_t i = 0; i < m_count; i++) {
			sum += m_intervals[i];
		}
		return std::clamp<uint64_t>(sum / m_count / 2, 2'000'000, 50'000'000);
	}

private:
	static constexpr uint64_t kGapNs = 100'000'000;
	std::array<uint64_t, 8> m_intervals {};
	size_t                  m_next  = 0;
	size_t                  m_count = 0;
	uint64_t                m_last  = 0;
};

// Whether showing two frames per game frame can work on this display. Without vertical sync the
// presenter paces by itself. With it (FIFO) each present waits for a refresh, so the display must
// refresh at least twice per game frame, or the game would be slowed down; an unknown refresh
// rate (0) refuses for the same reason. One hertz of tolerance (119.88 Hz for 60 fps).
[[nodiscard]] constexpr bool DisplayAllows(bool fifo, uint32_t refresh_mhz, uint32_t game_hz) {
	return !fifo || (refresh_mhz != 0 && uint64_t {refresh_mhz} + 1000 >= uint64_t {2000} * game_hz);
}

// AMD's interpolation keeps the previous frame: its frame number goes up by one per dispatch, and
// a dispatch whose predecessor is not the frame shown just before it has to reset the history.
class Continuity {
public:
	struct Step {
		uint64_t frame_id;
		bool     reset;
	};
	// The new main frame is dispatched; game_frame is the title's frame counter of its capture.
	[[nodiscard]] Step Next(uint32_t game_frame) {
		const bool reset = m_broken || game_frame != m_game_frame + 1u;
		m_broken         = false;
		m_game_frame     = game_frame;
		return {++m_frame_id, reset};
	}
	// A new main frame was shown without a dispatch.
	void Break() { m_broken = true; }

private:
	uint64_t m_frame_id   = 0;
	uint32_t m_game_frame = 0;
	bool     m_broken     = true;
};

// What a presented frame finds in the capture ring.
enum class Lookup : uint8_t { Found, NoInputs, Lost };

// wanted: the capture the frame was prepared after (0: none); previous: the one of the frame
// before it; in_slot: the serial now in SlotOf(wanted).
[[nodiscard]] constexpr Lookup Find(uint64_t wanted, uint64_t previous, uint64_t in_slot) {
	if (wanted == 0 || wanted == previous) {
		return Lookup::NoInputs; // the title's pass did not run for this frame
	}
	return in_slot == wanted ? Lookup::Found : Lookup::Lost; // overwritten or the ring was remade
}

// The time between two captures as AMD wants it, in milliseconds.
[[nodiscard]] constexpr float FrameTimeMs(uint64_t previous_ns, uint64_t now_ns) {
	if (previous_ns == 0 || now_ns <= previous_ns) {
		return 16.7f;
	}
	return std::clamp(static_cast<float>(now_ns - previous_ns) / 1.0e6f, 1.0f, 250.0f);
}

enum class Outcome : uint8_t {
	Generated,
	Primed,   // dispatched with a reset: nothing to show yet
	NoInputs, // no capture for this frame
	Lost,     // its capture is gone
	Blank,    // a blank frame
	Display,  // DisplayAllows refused
	Format,   // a frame format or device feature the effect cannot take
	Error,    // AMD's contexts failed, now or before
	Count
};

struct Stats {
	std::array<uint32_t, static_cast<size_t>(Outcome::Count)> counts {};

	void Add(Outcome outcome) { counts[static_cast<size_t>(outcome)]++; }
	// "<frames> frames, <n> generated[, <name> <n>...]"; empty when no frame came since the last.
	[[nodiscard]] std::string Take() {
		static constexpr std::array<const char*, static_cast<size_t>(Outcome::Count)> NAMES {
		    "generated", "primed", "no inputs", "lost", "blank", "display", "format", "error"};
		uint32_t    total = 0;
		std::string others;
		for (size_t i = 0; i < counts.size(); i++) {
			total += counts[i];
			if (i != 0 && counts[i] != 0) {
				others += ", " + std::string(NAMES[i]) + " " + std::to_string(counts[i]);
			}
		}
		if (total == 0) {
			return {};
		}
		auto text = std::to_string(total) + " frames, " + std::to_string(counts[0]) + " generated" + others;
		counts    = {};
		return text;
	}
};

} // namespace Libs::Graphics::FrameGen

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FRAMEGENLOGIC_H_
