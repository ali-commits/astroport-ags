#include "streaming/protocol.h"

#include <algorithm>

namespace Streaming {
namespace {
bool Unsigned(const Json& value, uint64_t minimum, uint64_t maximum) {
	if (!value.is_number_unsigned() && !value.is_number_integer()) return false;
	if (value.is_number_integer() && !value.is_number_unsigned() && value.get<int64_t>() < 0)
		return false;
	const auto n = value.get<uint64_t>();
	return n >= minimum && n <= maximum;
}
} // namespace
bool ValidateHello(const Json& p, size_t fd_count, PoolDescriptor& output) noexcept {
	for (const char* key:
	     {"type", "version", "frames", "epoch", "width", "height", "stride", "fourcc", "gpu_uuid", "slots",
	      "allocation_sizes", "offset", "dedicated", "transport"})
		if (!p.is_object() || !p.contains(key)) return false;
	if (fd_count != 9 || p.at("type") != "hello" ||
	    !Unsigned(p.at("version"), ProtocolVersion, ProtocolVersion) || p.at("frames") != true ||
	    !Unsigned(p.at("epoch"), 1, UINT64_MAX) ||
	    !Unsigned(p.at("width"), 2, 8192) || !Unsigned(p.at("height"), 2, 8192) ||
	    !Unsigned(p.at("stride"), 4096, 268435456) || p.at("fourcc") != "BGRA" ||
	    !Unsigned(p.at("slots"), 3, 3) || !Unsigned(p.at("offset"), 0, 0) ||
	    !p.at("dedicated").is_boolean() || p.at("dedicated") != true ||
	    p.at("transport") != "vulkan-buffer-fd")
		return false;
	PoolDescriptor d;
	d.epoch  = p.at("epoch");
	d.width  = p.at("width");
	d.height = p.at("height");
	d.stride = p.at("stride");
	if (d.stride % 4096 != 0 || uint64_t(d.stride) < uint64_t(d.width) * 4) return false;
	if (!p.at("gpu_uuid").is_string()) return false;
	d.gpu_uuid = p.at("gpu_uuid").get<std::string>();
	if (d.gpu_uuid.size() != 32 || !std::all_of(d.gpu_uuid.begin(), d.gpu_uuid.end(), [](char c) {
		    return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
	    }))
		return false;
	auto& sizes = p.at("allocation_sizes");
	if (!sizes.is_array() || sizes.size() != 3) return false;
	for (size_t i = 0; i < SlotCount; i++) {
		if (!Unsigned(sizes[i], uint64_t(d.stride) * d.height, 268435456)) return false;
		d.allocation_sizes[i] = sizes[i];
	}
	output = std::move(d);
	return true;
}
std::optional<Lease> LeaseRing::TryAcquire() {
	std::unique_lock lock(m_mutex, std::try_to_lock);
	if (!lock.owns_lock()) return {};
	if (!m_connected || m_sequence == UINT64_MAX) return {};
	for (uint32_t i = 0; i < SlotCount; i++)
		if (m_leased[i] == 0) {
			m_leased[i] = ++m_sequence;
			Lease lease {m_epoch, m_sequence, i, m_used[i]};
			m_used[i] = true;
			return lease;
		}
	return {};
}
bool LeaseRing::Release(uint64_t epoch, uint32_t slot, uint64_t sequence) {
	std::lock_guard lock(m_mutex);
	if (!m_connected || epoch != m_epoch || slot >= SlotCount || sequence == 0 ||
	    m_leased[slot] != sequence)
		return false;
	m_leased[slot] = 0;
	return true;
}
void LeaseRing::Disconnect() {
	std::lock_guard lock(m_mutex);
	m_connected = false;
}
} // namespace Streaming
