#include "graphics/host_gpu/imageMemoryPressure.h"

#include <gtest/gtest.h>
#include <string>

namespace {

using namespace Libs::Graphics::ImageMemoryPressure;

constexpr uint64_t MIB = uint64_t {1} << 20u;

enum class Result { Ok, Oom, Other };

// An allocator whose first `strict_failures` strict attempts are out of memory; S / R: a strict /
// relaxed attempt, r: retained objects released.
struct Fake {
	int         strict_failures = 0;
	bool        retained        = true;
	Result      relaxed         = Result::Ok;
	Result      failure         = Result::Oom;
	std::string trace;
	bool        spilled = false;

	Result Run(State& state, uint64_t now_ms, uint64_t size, bool fallback) {
		return Libs::Graphics::ImageMemoryPressure::Run(
		    state, now_ms, size, fallback, Result::Oom,
		    [this](bool strict) {
			    trace += strict ? 'S' : 'R';
			    if (!strict) {
				    return relaxed;
			    }
			    return strict_failures-- > 0 ? failure : Result::Ok;
		    },
		    [this] {
			    trace += 'r';
			    return retained;
		    },
		    &spilled);
	}
};

TEST(ImageMemoryPressure, FittingImageIsOneStrictAttempt) {
	State state;
	Fake  fake;
	EXPECT_EQ(fake.Run(state, 1000, MIB, true), Result::Ok);
	EXPECT_EQ(fake.trace, "S");
	EXPECT_FALSE(fake.spilled);
	EXPECT_EQ(state.wanted.load(), 0u);
	EXPECT_FALSE(state.CoolingDown(1000, MIB));
}

TEST(ImageMemoryPressure, ReleasedRetainedObjectsMakeRoom) {
	State state;
	Fake  fake {.strict_failures = 1};
	EXPECT_EQ(fake.Run(state, 1000, MIB, true), Result::Ok);
	EXPECT_EQ(fake.trace, "SrS");
	EXPECT_FALSE(fake.spilled);
	EXPECT_EQ(state.wanted.load(), 0u);
}

TEST(ImageMemoryPressure, FullMemoryAsksTheCacheThenSpills) {
	State state;
	Fake  fake {.strict_failures = 2};
	EXPECT_EQ(fake.Run(state, 1000, MIB, true), Result::Ok);
	EXPECT_EQ(fake.trace, "SrSR");
	EXPECT_TRUE(fake.spilled);
	EXPECT_EQ(state.wanted.load(), MIN_WANTED); // at least 64 MiB
	Fake big {.strict_failures = 2};
	EXPECT_EQ(big.Run(state, 2000, 200 * MIB, true), Result::Ok);
	EXPECT_EQ(state.wanted.load(), MIN_WANTED + 200 * MIB);
}

TEST(ImageMemoryPressure, NothingRetainedSkipsTheRetry) {
	State state;
	Fake  fake {.strict_failures = 1, .retained = false};
	EXPECT_EQ(fake.Run(state, 1000, MIB, true), Result::Ok);
	EXPECT_EQ(fake.trace, "SrR");
}

TEST(ImageMemoryPressure, FallbackOffNeverRelaxes) {
	State state;
	Fake  fake {.strict_failures = 2};
	EXPECT_EQ(fake.Run(state, 1000, MIB, false), Result::Oom);
	EXPECT_EQ(fake.trace, "SrS");
	EXPECT_FALSE(fake.spilled);
	EXPECT_EQ(state.wanted.load(), MIN_WANTED); // the cache is still asked
	EXPECT_FALSE(state.CoolingDown(1001, MIB));
	Fake next {.strict_failures = 0};
	EXPECT_EQ(next.Run(state, 1001, MIB, false), Result::Ok);
	EXPECT_EQ(next.trace, "S");
}

TEST(ImageMemoryPressure, OtherFailuresAreNotMemoryPressure) {
	State state;
	Fake  fake {.strict_failures = 1, .failure = Result::Other};
	EXPECT_EQ(fake.Run(state, 1000, MIB, true), Result::Other);
	EXPECT_EQ(fake.trace, "S");
	EXPECT_EQ(state.wanted.load(), 0u);
}

TEST(ImageMemoryPressure, CooldownSkipsTheFailingStepsBySize) {
	State state;
	Fake  first {.strict_failures = 2};
	(void)first.Run(state, 1000, 8 * MIB, true);
	// Within 250 ms: as large or larger goes straight to the relaxed allocation.
	Fake same;
	EXPECT_EQ(same.Run(state, 1249, 8 * MIB, true), Result::Ok);
	EXPECT_EQ(same.trace, "R");
	EXPECT_FALSE(same.spilled);
	// A smaller one may still fit: strict.
	Fake small;
	EXPECT_EQ(small.Run(state, 1100, 8 * MIB - 1, true), Result::Ok);
	EXPECT_EQ(small.trace, "S");
	// Expired.
	Fake late;
	EXPECT_EQ(late.Run(state, 1250, 64 * MIB, true), Result::Ok);
	EXPECT_EQ(late.trace, "S");
}

TEST(ImageMemoryPressure, CooldownOutOfMemoryStillReleasesRetainedObjects) {
	State state;
	state.StartCooldown(1000, MIB);
	Fake fake {.strict_failures = 1, .relaxed = Result::Oom};
	EXPECT_EQ(fake.Run(state, 1100, MIB, true), Result::Ok);
	EXPECT_EQ(fake.trace, "RSrS");
	EXPECT_FALSE(fake.spilled);
}

TEST(ImageMemoryPressure, CooldownMinimumShrinksAndDeadlineExtends) {
	State state;
	state.StartCooldown(1000, 8 * MIB);
	EXPECT_TRUE(state.CoolingDown(1100, 8 * MIB));
	EXPECT_FALSE(state.CoolingDown(1100, 2 * MIB));
	// A smaller request fails during the cooldown: the minimum shrinks, the deadline moves.
	Fake small {.strict_failures = 2};
	(void)small.Run(state, 1100, 2 * MIB, true);
	EXPECT_EQ(small.trace, "SrSR");
	EXPECT_TRUE(state.CoolingDown(1349, 2 * MIB));
	EXPECT_FALSE(state.CoolingDown(1350, 2 * MIB));
	// A larger failure during a cooldown keeps the smaller minimum.
	state.StartCooldown(1200, 32 * MIB);
	EXPECT_TRUE(state.CoolingDown(1449, 2 * MIB));
	// After expiry the minimum is the new request's size, not the smaller of the two.
	state.StartCooldown(5000, 32 * MIB);
	EXPECT_FALSE(state.CoolingDown(5001, 2 * MIB));
	EXPECT_TRUE(state.CoolingDown(5001, 32 * MIB));
}

} // namespace

int main(int argc, char** argv) {
	testing::InitGoogleTest(&argc, argv);
	return RUN_ALL_TESTS();
}
