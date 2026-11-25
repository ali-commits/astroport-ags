#include "streaming/transport.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstring>
#if defined(__linux__)
#include <fcntl.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#endif
namespace Streaming {
Packet::~Packet() {
#if defined(__linux__)
	for (int fd: fds)
		if (fd >= 0) close(fd);
#endif
}
#if defined(__linux__)
bool SameUser(int socket) {
	ucred     peer {};
	socklen_t size = sizeof(peer);
	return getsockopt(socket, SOL_SOCKET, SO_PEERCRED, &peer, &size) == 0 && size == sizeof(peer) &&
	       peer.uid == geteuid();
}
ReceiveResult ReceivePacket(int socket, Packet& packet, size_t expected_fds) {
	char                  bytes[PacketLimit];
	alignas(cmsghdr) char ancillary[CMSG_SPACE(sizeof(int) * 16)] {};
	iovec                 iov {bytes, sizeof(bytes)};
	msghdr                message {};
	message.msg_iov        = &iov;
	message.msg_iovlen     = 1;
	message.msg_control    = ancillary;
	message.msg_controllen = sizeof(ancillary);
	const auto count       = recvmsg(socket, &message, MSG_DONTWAIT | MSG_CMSG_CLOEXEC);
	if (count < 0)
		return errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR ? ReceiveResult::Again
		                                                                 : ReceiveResult::Closed;
	bool bad = false;
	for (auto* c = CMSG_FIRSTHDR(&message); c; c = CMSG_NXTHDR(&message, c)) {
		if (c->cmsg_level != SOL_SOCKET || c->cmsg_type != SCM_RIGHTS ||
		    c->cmsg_len < CMSG_LEN(0)) {
			bad = true;
			continue;
		}
		const size_t size = c->cmsg_len - CMSG_LEN(0);
		if (size % sizeof(int) != 0) bad = true;
		auto* fds = reinterpret_cast<int*>(CMSG_DATA(c));
		for (size_t i = 0; i < size / sizeof(int); i++)
			packet.fds.push_back(fds[i]);
	}
	if (count == 0) return packet.fds.empty() ? ReceiveResult::Closed : ReceiveResult::Invalid;
	if (bad || (message.msg_flags & (MSG_TRUNC | MSG_CTRUNC)) || packet.fds.size() != expected_fds)
		return ReceiveResult::Invalid;
	packet.json = Json::parse(bytes, bytes + count, nullptr, false);
	return packet.json.is_object() ? ReceiveResult::Packet : ReceiveResult::Invalid;
}
bool SendPacket(int socket, const Json& packet, const std::array<int, 9>* fds) {
	if (!packet.is_object()) return false;
	const std::string data = packet.dump();
	if (data.size() > PacketLimit) return false;
	iovec  iov {const_cast<char*>(data.data()), data.size()};
	msghdr message {};
	message.msg_iov    = &iov;
	message.msg_iovlen = 1;
	alignas(cmsghdr) char ancillary[CMSG_SPACE(sizeof(int) * 9)] {};
	if (fds) {
		message.msg_control    = ancillary;
		message.msg_controllen = sizeof(ancillary);
		auto* c                = CMSG_FIRSTHDR(&message);
		c->cmsg_level          = SOL_SOCKET;
		c->cmsg_type           = SCM_RIGHTS;
		c->cmsg_len            = CMSG_LEN(sizeof(int) * 9);
		std::memcpy(CMSG_DATA(c), fds->data(), sizeof(int) * 9);
	}
	return sendmsg(socket, &message, MSG_DONTWAIT | MSG_NOSIGNAL) ==
	       static_cast<ssize_t>(data.size());
}
static int Connect(const std::string& path) {
	if (path.empty() || path.size() >= sizeof(sockaddr_un::sun_path) ||
	    path.find('\0') != std::string::npos) return -1;
	const auto slash = path.rfind('/');
	if (slash == std::string::npos) return -1;
	struct stat status {};
	if (lstat(path.c_str(), &status) != 0 || !S_ISSOCK(status.st_mode) ||
	    status.st_uid != geteuid() || (status.st_mode & 077) != 0)
		return -1;
	if (lstat(path.substr(0, slash).c_str(), &status) != 0 || !S_ISDIR(status.st_mode) ||
	    status.st_uid != geteuid() || (status.st_mode & 077) != 0)
		return -1;
	int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
	if (fd < 0) return -1;
	sockaddr_un address {};
	address.sun_family = AF_UNIX;
	std::memcpy(address.sun_path, path.c_str(), path.size() + 1);
	if (connect(fd, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 0 || !SameUser(fd)) {
		close(fd);
		return -1;
	}
	return fd;
}
#else
bool SameUser(int) {
	return false;
}
ReceiveResult ReceivePacket(int, Packet&, size_t) {
	return ReceiveResult::Closed;
}
bool SendPacket(int, const Json&, const std::array<int, 9>*) {
	return false;
}
#endif
Transport::Transport(std::string path, Completion complete, MetricsHooks metrics,
                     SettingsHook settings, bool metrics_only)
    : m_path(std::move(path)), m_metrics(std::move(metrics)), m_complete(std::move(complete)),
      m_settings(std::move(settings)), m_metrics_only(metrics_only),
      m_worker([this](std::stop_token stop) { Run(stop); }) {}
Transport::~Transport() {
	m_worker.request_stop();
	m_worker.join();
}
void Transport::SetPool(PoolDescriptor descriptor, std::array<int, 9> fds,
                        std::shared_ptr<LeaseRing> ring, uint64_t generation) {
	std::lock_guard lock(m_mutex);
	if (MetricsOnly() || generation == 0 || generation != Generation()) return;
	if (m_ring) m_ring->Disconnect();
	m_descriptor      = std::move(descriptor);
	m_fds             = fds;
	m_ring            = std::move(ring);
	m_pool_generation = generation;
	for (auto& pending: m_pending)
		pending.published.store(false, std::memory_order_relaxed);
	m_active_epoch.store(m_descriptor.epoch, std::memory_order_release);
}
void Transport::SetCompletion(Completion complete) {
	std::lock_guard lock(m_mutex); // the worker calls it under this lock
	m_complete = std::move(complete);
}
void Transport::Publish(PendingFrame frame) {
	if (frame.lease.slot >= SlotCount ||
	    m_active_epoch.load(std::memory_order_acquire) != frame.lease.epoch) {
		Drop();
		return;
	}
	auto& pending = m_pending[frame.lease.slot];
	pending.frame = frame;
	pending.published.store(true, std::memory_order_release);
}
void Transport::Reconnect() {
	m_reconnect.store(true, std::memory_order_release);
}
void Transport::ClearPool() {
	m_active_epoch = 0;
	if (m_ring) m_ring->Disconnect();
	m_ring.reset();
	m_fds = {}; // The exporter owns these descriptors.
	m_pool_generation = 0;
	for (auto& pending: m_pending)
		pending.published.store(false, std::memory_order_relaxed);
}

void Transport::SetMetricsOnly(bool metrics_only) {
	std::lock_guard lock(m_mutex);
	if (m_attached || metrics_only == MetricsOnly()) return;
	m_metrics_only.store(metrics_only, std::memory_order_release);
	if (metrics_only) ClearPool();
}

void Transport::Run(std::stop_token stop) {
#if defined(__linux__)
	int socket = -1;
	uint64_t generation = 0, last_apply_id = 0;
	bool welcomed = false, hello_sent = false;
	// Zero denotes a frame-less hello; a frame hello always has a positive epoch.
	uint64_t offered_epoch = 0;
	auto retry = std::chrono::steady_clock::now();
	MetricsSubscription subscription;
	MetricsPacer metrics_pacer;
	UnknownTypeLog unknown_types;
	bool logged_early_apply = false, logged_bad_apply_id = false, logged_refusal = false;
	const auto version_matches = [](const Json& p) {
		const auto version = p.find("version");
		return version != p.end() && version->is_number_integer() && *version == ProtocolVersion;
	};
	const auto set_subscription = [&](const MetricsSubscription& next) {
		const bool changed = next.keys != subscription.keys;
		subscription = next;
		if (changed && m_metrics.subscribed) m_metrics.subscribed(subscription.keys);
	};
	const auto disconnect = [&] {
		if (socket >= 0) close(socket);
		socket = -1;
		welcomed = hello_sent = false;
		offered_epoch = 0;
		last_apply_id = 0;
		m_attached = false;
		{
			std::lock_guard lock(m_mutex);
			m_generation = 0;
			ClearPool();
		}
		set_subscription({});
		metrics_pacer.Reset();
		retry = std::chrono::steady_clock::now() + std::chrono::seconds(1);
	};

	while (!stop.stop_requested()) {
		if (m_reconnect.exchange(false)) disconnect();
		const auto now = std::chrono::steady_clock::now();
		if (socket < 0) {
			if (now >= retry) {
				socket = Connect(m_path);
				retry = now + std::chrono::seconds(1);
			}
			if (socket < 0) {
				std::this_thread::sleep_for(std::chrono::milliseconds(10));
				continue;
			}
		}
		pollfd poller {socket, POLLIN, 0};
		if ((poll(&poller, 1, 2) < 0 && errno != EINTR) ||
		    (poller.revents & (POLLERR | POLLHUP | POLLNVAL))) {
			disconnect();
			continue;
		}
		bool invalid = false;
		for (size_t count = 0; count < 32; count++) {
			Packet packet;
			const auto result = ReceivePacket(socket, packet);
			if (result == ReceiveResult::Again) break;
			const auto& p = packet.json;
			const auto type_field = p.find("type");
			if (result != ReceiveResult::Packet || type_field == p.end() || !type_field->is_string()) {
				invalid = true;
				break;
			}
			const auto& type = type_field->get_ref<const std::string&>();
			if (type == "settings.apply" && !m_attached) {
				if (!logged_early_apply) {
					std::fprintf(stderr, "AGS session: ignoring settings.apply before accepted\n");
					logged_early_apply = true;
				}
				continue;
			}
			if (!welcomed) {
				if (type != "welcome" || !version_matches(p)) {
					invalid = true;
					break;
				}
				welcomed = true;
				m_generation = ++generation;
				continue;
			}
			if (type == "accepted") {
				const bool correct_mode =
				    offered_epoch == 0
				        ? p.contains("frames") && p["frames"] == false
				        : p.contains("epoch") && p["epoch"].is_number_unsigned() &&
				              p["epoch"] == offered_epoch && p.contains("transport") &&
				              p["transport"] == "vulkan-buffer-fd";
				if (!hello_sent || m_attached || !version_matches(p) || !correct_mode) {
					invalid = true;
					break;
				}
				m_attached = true;
			} else if (type == "refused") {
				if (!hello_sent || offered_epoch == 0 || m_attached || !version_matches(p)) {
					invalid = true;
					break;
				}
				if ((!p.contains("reason") || p["reason"] != "local") && !logged_refusal) {
					std::fprintf(stderr, "AGS session: frame hello refused, falling back to frame-less\n");
					logged_refusal = true;
				}
				SetMetricsOnly(true);
				offered_epoch = 0;
				hello_sent = false;
			} else if (type == "error" || type == "welcome") {
				invalid = true;
				break;
			} else if (type == "metrics.subscribe") {
				MetricsSubscription next;
				if (ParseMetricsSubscribe(p, next)) {
					set_subscription(next);
				} else if (unknown_types.First("metrics.subscribe(malformed)")) {
					std::fprintf(stderr, "AGS session: ignoring malformed metrics.subscribe\n");
				}
			} else if (type == "settings.apply") {
				const auto id = p.find("id");
				if (id == p.end() || !id->is_number_unsigned() || id->get<uint64_t>() <= last_apply_id) {
					if (!logged_bad_apply_id) {
						std::fprintf(stderr, "AGS session: ignoring invalid settings.apply id\n");
						logged_bad_apply_id = true;
					}
					continue;
				}
				last_apply_id = id->get<uint64_t>();
				if (!m_settings) continue;
				const auto old_fps = offered_epoch == 0 ? ReadLiveOutput().fps : 0;
				const auto reply = m_settings(p);
				if (!SendPacket(socket, reply)) {
					invalid = true;
					break;
				}
				const auto applied = reply.find("applied");
				if (offered_epoch == 0 && applied != reply.end() && applied->is_object() &&
				    (applied->contains("display.resolution") ||
				     (applied->contains("display.fps") && (*applied)["display.fps"] != old_fps))) {
					const auto output = ReadLiveOutput();
					if (!SendPacket(socket, {{"type", "output"},
					                         {"width", std::clamp(output.width, 2u, 8192u)},
					                         {"height", std::clamp(output.height, 2u, 8192u)},
					                         {"fps", std::clamp(output.fps, 1u, 360u)}})) {
						invalid = true;
						break;
					}
				}
			} else if (type == "session.state") {
				const auto state = p.find("state");
				if (state != p.end() && (*state == "paused" || *state == "resumed"))
					std::fprintf(stderr, "AGS session: %s\n", state->get_ref<const std::string&>().c_str());
			} else if (type == "release" && offered_epoch != 0) {
				if (!m_attached || !p.contains("epoch") || !p.contains("slot") ||
				    !p.contains("sequence") || !p["epoch"].is_number_unsigned() ||
				    !p["slot"].is_number_unsigned() || !p["sequence"].is_number_unsigned() ||
				    p["slot"].get<uint64_t>() >= SlotCount) {
					invalid = true;
					break;
				}
				std::lock_guard lock(m_mutex);
				if (m_ring) m_ring->Release(p["epoch"], p["slot"], p["sequence"]);
			} else if (!IsKnownPacketType(type) && unknown_types.First(type)) {
				const bool printable = std::all_of(type.begin(), type.end(),
				    [](unsigned char c) { return c >= 0x20 && c < 0x7f; });
				std::fprintf(stderr, "AGS session: ignoring unknown packet type \"%.40s\"\n",
				             printable ? type.c_str() : "<non-printable>");
			}
		}
		if (invalid) {
			disconnect();
			continue;
		}
		if (welcomed && !hello_sent) {
			std::lock_guard lock(m_mutex);
			if (MetricsOnly()) {
				hello_sent = SendPacket(socket, {{"type", "hello"}, {"version", ProtocolVersion},
				                                 {"frames", false}});
			} else if (m_ring && m_pool_generation == generation) {
				const auto& d = m_descriptor;
				const Json hello = {{"type", "hello"},
				                    {"version", ProtocolVersion},
				                    {"frames", true},
				                    {"epoch", d.epoch},
				                    {"width", d.width},
				                    {"height", d.height},
				                    {"stride", d.stride},
				                    {"fourcc", "BGRA"},
				                    {"gpu_uuid", d.gpu_uuid},
				                    {"slots", SlotCount},
				                    {"allocation_sizes", d.allocation_sizes},
				                    {"offset", 0},
				                    {"dedicated", true},
				                    {"transport", "vulkan-buffer-fd"}};
				if (SendPacket(socket, hello, &m_fds)) {
					hello_sent = true;
					offered_epoch = d.epoch;
				}
			}
		}
		if (m_attached && offered_epoch != 0) {
			std::lock_guard lock(m_mutex);
			for (size_t count = 0; count < SlotCount; count++) {
				Mailbox* next = nullptr;
				for (auto& pending: m_pending) {
					if (pending.published.load(std::memory_order_acquire) &&
					    (next == nullptr ||
					     pending.frame.lease.sequence < next->frame.lease.sequence))
						next = &pending;
				}
				if (next == nullptr || !m_complete || !m_complete(next->frame.tick)) break;
				const auto& pending = next->frame;
				Json        frame   = {{"type", "frame"},
				                       {"epoch", pending.lease.epoch},
				                       {"slot", pending.lease.slot},
				                       {"sequence", pending.lease.sequence},
				                       {"pts_ns", pending.pts_ns}};
				if (!SendPacket(socket, frame)) break;
				next->published.store(false, std::memory_order_release);
			}
		}
		if (m_attached && subscription.keys != 0 && m_metrics.sample &&
		    metrics_pacer.Due(now, subscription.interval_ms)) {
			metrics_pacer.Attempted(now);
			if (const auto message = BuildMetricsMessage(
			        subscription.keys, m_metrics.sample(subscription.keys), MonotonicMs()))
				SendPacket(socket, *message);
		}
	}
	disconnect();
#else
	(void)stop;
#endif
}
} // namespace Streaming
