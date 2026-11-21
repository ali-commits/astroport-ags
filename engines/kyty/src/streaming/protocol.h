#pragma once

#include <array>
#include <cstdint>
#include <mutex>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace Streaming {
constexpr int    ProtocolVersion = 2;
constexpr size_t PacketLimit = 8192;
constexpr size_t SlotCount   = 3;
using Json                   = nlohmann::json;
struct PoolDescriptor {
	uint64_t                        epoch = 0;
	uint32_t                        width = 0, height = 0, stride = 0;
	std::array<uint64_t, SlotCount> allocation_sizes {};
	std::string                     gpu_uuid;
};
bool ValidateHello(const Json& packet, size_t fd_count, PoolDescriptor& output) noexcept;
// Packet types this emulator understands.
// Any other type from the host is ignored instead of dropping the connection.
[[nodiscard]] inline bool IsKnownPacketType(std::string_view type) noexcept {
	for (const auto known: {"welcome", "accepted", "refused", "error", "release",
	                        "metrics.subscribe", "settings.apply", "session.state"})
		if (type == known) return true;
	return false;
}
// Remembers which unknown packet types were already reported, so each is logged once.
class UnknownTypeLog {
public:
	// True the first time `type` is seen. Past a small cap of distinct types nothing is reported.
	[[nodiscard]] bool First(std::string_view type) {
		for (const auto& seen: m_seen)
			if (seen == type) return false;
		if (m_seen.size() >= 16) return false;
		m_seen.emplace_back(type);
		return true;
	}

private:
	std::vector<std::string> m_seen;
};
struct Lease {
	uint64_t epoch, sequence;
	uint32_t slot;
	bool     recycled;
};
class LeaseRing {
public:
	explicit LeaseRing(uint64_t epoch): m_epoch(epoch) {}
	std::optional<Lease> TryAcquire();
	bool                 Release(uint64_t epoch, uint32_t slot, uint64_t sequence);
	void                 Disconnect();

private:
	std::mutex                      m_mutex;
	uint64_t                        m_epoch, m_sequence = 0;
	bool                            m_connected = true;
	std::array<uint64_t, SlotCount> m_leased {};
	std::array<bool, SlotCount>     m_used {};
};
} // namespace Streaming
