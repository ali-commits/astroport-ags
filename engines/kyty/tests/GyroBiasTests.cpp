#include "libs/gyroBias.h"

#include <gtest/gtest.h>

namespace {

using Libs::Controller::GyroBias;

constexpr uint64_t      STEP_US = 5000; // 200 Hz
constexpr GyroBias::Vec3 BIAS {0.0873f, -0.0157f, -0.0332f}; // 5.0, -0.9, -1.9 deg/s
constexpr GyroBias::Vec3 UP {0.0f, 1.0f, 0.0f};

// +-0.05 deg/s of deterministic noise.
GyroBias::Vec3 Noisy(const GyroBias::Vec3& value, uint64_t sample) {
	const float noise = (static_cast<float>(sample * 37 % 21) - 10.0f) * 0.0000873f;
	return {value[0] + noise, value[1] - noise, value[2] + noise};
}

TEST(GyroBias, ConstantBiasIsLearnedWithin1300MsAndRemoved) {
	GyroBias bias;
	uint64_t learned_at = 0;
	for (uint64_t i = 0; i * STEP_US <= 1300000; i++) {
		if (bias.Add(Noisy(BIAS, i), UP, i * STEP_US) && learned_at == 0) {
			learned_at = i * STEP_US;
		}
		if (learned_at == 0) {
			EXPECT_FALSE(bias.Valid());
			EXPECT_EQ(bias.Offset(), (GyroBias::Vec3 {}));
		}
	}
	ASSERT_TRUE(bias.Valid());
	EXPECT_EQ(learned_at, 1250000u); // 50 samples to settle gravity, then a one-second window
	for (int i = 0; i < 3; i++) {
		EXPECT_NEAR(bias.Offset()[i], BIAS[i], 0.0002f);
	}
}

TEST(GyroBias, OffsetIsRefreshedEverySecondAtRest) {
	GyroBias bias;
	int      windows = 0;
	for (uint64_t i = 0; i * STEP_US <= 3300000; i++) {
		// The offset drifts (temperature) slowly enough to stay inside every window.
		const GyroBias::Vec3 drifting {BIAS[0] + static_cast<float>(i) * 0.00001f, BIAS[1], BIAS[2]};
		windows += bias.Add(drifting, UP, i * STEP_US) ? 1 : 0;
	}
	EXPECT_EQ(windows, 3);
	EXPECT_GT(bias.Offset()[0], BIAS[0] + 0.004f);
}

TEST(GyroBias, RotationAboveTheThresholdNeverLearns) {
	GyroBias bias;
	for (uint64_t i = 0; i < 2000; i++) {
		// A turn that speeds up by 2 deg/s every second: no window lasts a second.
		const GyroBias::Vec3 turning {0.1f + static_cast<float>(i) * 0.000175f, 0.0f, 0.0f};
		EXPECT_FALSE(bias.Add(turning, UP, i * STEP_US));
	}
	EXPECT_FALSE(bias.Valid());
}

TEST(GyroBias, TimestampGapRestartsTheWindow) {
	GyroBias bias;
	uint64_t time = 0;
	for (; time < 1000000; time += STEP_US) {
		EXPECT_FALSE(bias.Add(BIAS, UP, time));
	}
	time += 150000; // lost reports, 250 ms before the window would have completed
	const uint64_t restart = time;
	for (; time < restart + 1000000; time += STEP_US) {
		EXPECT_FALSE(bias.Add(BIAS, UP, time)) << time;
	}
	EXPECT_TRUE(bias.Add(BIAS, UP, time));
	// A time that goes backwards restarts it too.
	EXPECT_FALSE(bias.Add(BIAS, UP, time + STEP_US));
	EXPECT_FALSE(bias.Add(BIAS, UP, time));
	for (uint64_t t = time + STEP_US; t < time + 1000000; t += STEP_US) {
		EXPECT_FALSE(bias.Add(BIAS, UP, t)) << t;
	}
	EXPECT_TRUE(bias.Add(BIAS, UP, time + 1000000));
}

TEST(GyroBias, TiltChangeRestartsTheWindow) {
	GyroBias bias;
	// The pad is tilted slowly (gravity moves 0.02 G a second) while the gyro reads a constant.
	for (uint64_t i = 0; i < 2000; i++) {
		const float          tilt = static_cast<float>(i) * 0.0001f;
		const GyroBias::Vec3 gravity {tilt, 1.0f - tilt, 0.0f};
		EXPECT_FALSE(bias.Add(BIAS, gravity, i * STEP_US)) << i;
	}
	EXPECT_FALSE(bias.Valid());
}

TEST(GyroBias, NanSampleNeverBecomesTheOffset) {
	GyroBias   bias;
	const auto nan = std::nanf("");
	for (uint64_t i = 0; i < 600; i++) {
		EXPECT_FALSE(bias.Add({nan, 0.0f, 0.0f}, UP, i * STEP_US));
	}
	EXPECT_FALSE(bias.Valid());
}

} // namespace

int main(int argc, char** argv) {
	testing::InitGoogleTest(&argc, argv);
	return RUN_ALL_TESTS();
}
