#pragma once
#include "streaming/metrics.h"
#include "streaming/protocol.h"

#include <atomic>
#include <functional>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>
namespace Streaming {
struct Packet {
	Json             json;
	std::vector<int> fds;
	Packet() = default;
	~Packet();
	Packet(const Packet&)            = delete;
	Packet& operator=(const Packet&) = delete;
};
enum class ReceiveResult { Packet, Again, Closed, Invalid };
ReceiveResult ReceivePacket(int socket, Packet& packet, size_t expected_fds = 0);
bool          SameUser(int socket);
bool          SendPacket(int socket, const Json& packet, const std::array<int, 9>* fds = nullptr);
// Task 4 supplies the current display state from Live().
struct OutputState {
	uint32_t width, height, fps;
};
OutputState ReadLiveOutput();

struct PendingFrame {
	Lease    lease;
	uint64_t tick, pts_ns;
};
class Transport {
public:
	using Completion   = std::function<bool(uint64_t)>;
	using SettingsHook = std::function<Json(const Json& apply)>;

	Transport(std::string path, Completion complete, MetricsHooks metrics, SettingsHook settings,
	          bool metrics_only);
	~Transport();
	uint64_t Generation() const { return m_generation.load(std::memory_order_acquire); }
	bool     Attached() const { return m_attached.load(std::memory_order_acquire); }
	void SetPool(PoolDescriptor descriptor, std::array<int, 9> fds, std::shared_ptr<LeaseRing> ring,
	             uint64_t generation);
	void Publish(PendingFrame frame);
	void Reconnect();
	// A frame-less producer can still be attached for metrics and settings.
	void     SetMetricsOnly(bool metrics_only = true);
	bool     MetricsOnly() const { return m_metrics_only.load(std::memory_order_acquire); }
	uint64_t Dropped() const { return m_dropped.load(std::memory_order_relaxed); }
	void     Drop() { m_dropped.fetch_add(1, std::memory_order_relaxed); }

private:
	void                       Run(std::stop_token stop);
	void                       ClearPool(); // Caller holds m_mutex.
	std::string                m_path;
	MetricsHooks               m_metrics;
	Completion                 m_complete;
	SettingsHook               m_settings;
	std::mutex                 m_mutex;
	PoolDescriptor             m_descriptor;
	std::array<int, 9>         m_fds {};
	std::shared_ptr<LeaseRing> m_ring;
	struct Mailbox {
		std::atomic<bool> published {false};
		PendingFrame      frame {};
	};
	// A lease owns this slot until the worker has sent FRAME and processed its ACK.
	// Present publishes without acquiring the socket worker's mutex.
	std::array<Mailbox, SlotCount> m_pending;
	uint64_t                       m_pool_generation = 0;
	std::atomic<uint64_t>          m_generation {0}, m_dropped {0}, m_active_epoch {0};
	std::atomic<bool>              m_attached {false}, m_reconnect {false}, m_metrics_only {false};
	std::jthread                   m_worker;
};
} // namespace Streaming
