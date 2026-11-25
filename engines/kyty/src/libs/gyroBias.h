#ifndef LIBS_GYROBIAS_H_
#define LIBS_GYROBIAS_H_

#include <array>
#include <cmath>
#include <cstdint>

namespace Libs::Controller {

// Zero-rate offset of a gyroscope, learned while the pad lies still. Some pads report a constant
// angular velocity at rest (a Switch Pro Controller: about 5 deg/s on one axis; SDL reads its
// factory offset and never subtracts it), which the game sees as a steady rotation.
//
// Add() takes every gyro sample (rad/s) with the latest accelerometer reading (G) and the sensor
// time. A rest window is a run of samples with no time gap, every gyro axis within 1 deg/s of the
// window's first sample and smoothed gravity within 0.01 G of its value at the window's start.
// After one second the window's mean becomes the offset, so the offset follows temperature drift
// about once a second at rest.
//
// shortcut: a steady turn below 1 deg/s (or tilt below about 0.75 deg/s) held for a whole second
// is learned as offset; upgrade to an explicit "calibrate now" action if that ever shows.
class GyroBias {
public:
	using Vec3 = std::array<float, 3>;

	// Returns true when this sample completed a rest window (Offset() is new).
	bool Add(const Vec3& gyro, const Vec3& accel, uint64_t time_us) {
		for (int i = 0; i < 3; i++) {
			m_gravity[i] = m_gravity_samples == 0 ? accel[i] : m_gravity[i] + GRAVITY_WEIGHT * (accel[i] - m_gravity[i]);
		}
		if (m_gravity_samples < SETTLE_SAMPLES) {
			m_gravity_samples++;
			return false;
		}
		const bool resting = m_count != 0 && time_us >= m_last_time && time_us - m_last_time <= MAX_GAP_US &&
		                     Near(gyro, m_first, GYRO_TOLERANCE) && Near(m_gravity, m_start_gravity, GRAVITY_TOLERANCE);
		if (!resting) {
			m_count         = 0;
			m_sum           = {};
			m_first         = gyro;
			m_start_gravity = m_gravity;
			m_start_time    = time_us;
		}
		for (int i = 0; i < 3; i++) {
			m_sum[i] += static_cast<double>(gyro[i]);
		}
		m_count++;
		m_last_time = time_us;
		if (time_us - m_start_time < WINDOW_US) {
			return false;
		}
		for (int i = 0; i < 3; i++) {
			m_offset[i] = static_cast<float>(m_sum[i] / m_count);
		}
		m_valid = true;
		m_count = 0; // the next sample starts a new window
		return true;
	}

	[[nodiscard]] bool        Valid() const { return m_valid; }
	// Zero until the first window completed.
	[[nodiscard]] const Vec3& Offset() const { return m_offset; }

private:
	static constexpr float    GRAVITY_WEIGHT    = 0.02f;
	static constexpr uint32_t SETTLE_SAMPLES    = 50; // about 0.25 s at 200 Hz
	static constexpr uint64_t MAX_GAP_US        = 100000;
	static constexpr uint64_t WINDOW_US         = 1000000;
	static constexpr float    GYRO_TOLERANCE    = 0.0175f; // rad/s: 1 deg/s
	static constexpr float    GRAVITY_TOLERANCE = 0.01f;   // G: about 0.6 degrees of tilt

	static bool Near(const Vec3& a, const Vec3& b, float tolerance) {
		for (int i = 0; i < 3; i++) {
			if (!(std::fabs(a[i] - b[i]) <= tolerance)) { // a NaN is never near
				return false;
			}
		}
		return true;
	}

	Vec3                  m_gravity {};
	uint32_t              m_gravity_samples = 0;
	Vec3                  m_first {};
	Vec3                  m_start_gravity {};
	uint64_t              m_start_time = 0;
	uint64_t              m_last_time  = 0;
	std::array<double, 3> m_sum {};
	uint32_t              m_count = 0;
	Vec3                  m_offset {};
	bool                  m_valid = false;
};

} // namespace Libs::Controller

#endif /* LIBS_GYROBIAS_H_ */
