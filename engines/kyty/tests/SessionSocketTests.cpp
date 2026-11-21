#include "streaming/liveSettings.h"
#include "streaming/sessionEnv.h"
#include "streaming/transport.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

namespace {
using namespace Streaming;
using namespace std::chrono_literals;

void Check(bool ok, const char* message) {
	if (!ok) {
		std::fprintf(stderr, "SessionSocketTests: failed: %s\n", message);
		std::abort();
	}
}

int Poll(int fd, int timeout = 2000) {
	pollfd event {fd, POLLIN, 0};
	Check(poll(&event, 1, timeout) >= 0, "poll socket");
	return event.revents;
}

class Host {
public:
	Host() {
		char dir[] = "/tmp/kyty-session-XXXXXX";
		Check(mkdtemp(dir) != nullptr, "make private socket directory");
		m_dir = dir;
		path = m_dir + "/socket";
		m_listener = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
		Check(m_listener >= 0, "create seqpacket listener");
		sockaddr_un addr {};
		addr.sun_family = AF_UNIX;
		std::memcpy(addr.sun_path, path.c_str(), path.size() + 1);
		Check(bind(m_listener, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) == 0, "bind socket");
		Check(chmod(path.c_str(), 0600) == 0 && listen(m_listener, 1) == 0, "listen on private socket");
	}

	~Host() {
		if (peer >= 0) close(peer);
		close(m_listener);
		unlink(path.c_str());
		rmdir(m_dir.c_str());
	}

	void Welcome(int version = 2) {
		Check(Poll(m_listener) & POLLIN, "producer connects");
		peer = accept4(m_listener, nullptr, nullptr, SOCK_CLOEXEC);
		Check(peer >= 0 && SameUser(peer), "accept same-user producer");
		Send({{"type", "welcome"}, {"version", version}});
	}

	void Send(const Json& json) const {
		Check(SendPacket(peer, json), "send host packet");
	}

	void Receive(Packet& packet, size_t fds = 0) const {
		Check(Poll(peer) & POLLIN, "producer packet is readable");
		Check(ReceivePacket(peer, packet, fds) == ReceiveResult::Packet, "receive producer packet");
	}

	void Expect(const Json& expected) const {
		Packet packet;
		Receive(packet);
		Check(packet.json == expected, "producer packet matches");
	}

	void ExpectFramelessHello() const {
		Expect({{"type", "hello"}, {"version", 2}, {"frames", false}});
	}

	void AcceptFrameless(const Transport& transport) const {
		Send({{"type", "accepted"}, {"version", 2}, {"frames", false}});
		const auto until = std::chrono::steady_clock::now() + 2s;
		while (!transport.Attached() && std::chrono::steady_clock::now() < until)
			std::this_thread::sleep_for(1ms);
		Check(transport.Attached(), "producer attached after accepted");
	}

	void ConnectFrameless(const Transport& transport) {
		Welcome();
		ExpectFramelessHello();
		Check(!transport.Attached(), "hello alone does not attach");
		AcceptFrameless(transport);
	}

	void ExpectClosed() const {
		Check(Poll(peer) & (POLLIN | POLLHUP), "rejected connection closes");
		Packet packet;
		Check(ReceivePacket(peer, packet) == ReceiveResult::Closed, "no reply before close");
	}

	std::string path;
	int peer = -1;

private:
	std::string m_dir;
	int m_listener = -1;
};

Json Apply(uint64_t id, Json values) {
	return {{"type", "settings.apply"}, {"id", id}, {"values", std::move(values)}};
}

Json Applied(uint64_t id, Json applied = Json::object(), Json values = Json::object()) {
	return {{"type", "settings.applied"}, {"id", id}, {"applied", std::move(applied)},
	        {"values", std::move(values)}, {"refused", Json::object()}};
}

void TestEnvironment() {
	Check(SelectSessionSocket("/s", "2").path != nullptr, "protocol 2 selects the socket");
	Check(SelectSessionSocket("/s", "1").path == nullptr, "protocol 1 is ignored");
	Check(SelectSessionSocket("/s", nullptr).path == nullptr, "missing protocol is ignored");
	Check(SelectSessionSocket("", "2").path == nullptr, "empty socket is unset");
	Check(!FramesRequested("0") && FramesRequested(nullptr) && FramesRequested("1"),
	      "KYTY_STREAM_FRAMES");
}

void TestMetricsOnlyHandshake() {
	Host host;
	Transport transport(host.path, {}, {}, {}, true);
	host.ConnectFrameless(transport);
}

void TestWelcomeVersion1Rejected() {
	Host host;
	Transport transport(host.path, {}, {}, {}, true);
	host.Welcome(1);
	host.ExpectClosed();
}

void TestAcceptedVersion1Rejected() {
	Host host;
	Transport transport(host.path, {}, {}, {}, true);
	host.Welcome();
	host.ExpectFramelessHello();
	host.Send({{"type", "accepted"}, {"version", 1}, {"frames", false}});
	host.ExpectClosed();
	Check(!transport.Attached(), "accepted version 1 does not attach");
}

void TestSettingsApplyRoundTrip() {
	Host host;
	const auto request = Apply(7, {{"controller.haptics", 300}});
	const auto reply = Applied(7, Json::object(), {{"controller.haptics", 300}});
	Transport::SettingsHook settings = [&](const Json& apply) {
		Check(apply == request, "settings hook receives whole request");
		return reply;
	};
	Transport transport(host.path, {}, {}, settings, true);
	host.ConnectFrameless(transport);
	host.Send(request);
	host.Expect(reply);
}

void TestSettingsApplyBeforeAccepted() {
	Host host;
	Transport transport(host.path, {}, {}, [](const Json& apply) {
		Check(apply["id"] == 8, "no settings hook call before accepted");
		return Applied(8);
	}, true);
	host.Welcome();
	host.ExpectFramelessHello();
	host.Send(Apply(7, {{"controller.haptics", 300}}));
	Check(Poll(host.peer, 50) == 0, "no reply or disconnect before accepted");
	host.AcceptFrameless(transport);
	host.Send(Apply(8, {{"controller.haptics", 300}}));
	host.Expect(Applied(8));
}

void TestOutputAfterResolution() {
	Host host;
	Live().store({30, 1920, 1080, 1.0f, false, false});
	Transport transport(host.path, {}, {}, [](const Json& apply) {
		const auto& values = apply["values"];
		Live().Update([&](LiveState& state) {
			if (values.contains("display.resolution")) {
				state.width = 1280;
				state.height = 720;
			}
			if (values.contains("display.fps")) state.fps = values["display.fps"];
		});
		return Applied(apply["id"], values);
	}, true);
	host.ConnectFrameless(transport);
	host.Send(Apply(7, {{"display.resolution", "1280x720"}}));
	host.Expect(Applied(7, {{"display.resolution", "1280x720"}}));
	host.Expect({{"type", "output"}, {"width", 1280}, {"height", 720}, {"fps", 30}});
	host.Send(Apply(8, {{"display.fps", 30}}));
	host.Expect(Applied(8, {{"display.fps", 30}}));
	Check(Poll(host.peer, 50) == 0, "unchanged fps has no output");
	host.Send(Apply(9, {{"display.fps", 60}}));
	host.Expect(Applied(9, {{"display.fps", 60}}));
	host.Expect({{"type", "output"}, {"width", 1280}, {"height", 720}, {"fps", 60}});
}

void TestLocalRefusedFallsBackToFrameless() {
	Host host;
	const int fd = open("/dev/null", O_RDONLY | O_CLOEXEC);
	Check(fd >= 0, "open test descriptor");
	Transport transport(host.path, {}, {}, {}, false);
	host.Welcome();
	const auto until = std::chrono::steady_clock::now() + 2s;
	while (transport.Generation() == 0 && std::chrono::steady_clock::now() < until)
		std::this_thread::sleep_for(1ms);
	const auto generation = transport.Generation();
	Check(generation != 0, "welcome sets generation");
	const PoolDescriptor descriptor {3, 1280, 720, 8192,
	                                 {5898240, 5898240, 5898240},
	                                 "0123456789abcdef0123456789abcdef"};
	std::array<int, 9> fds;
	fds.fill(fd);
	const auto ring = std::make_shared<LeaseRing>(descriptor.epoch);
	transport.SetPool(descriptor, fds, ring, generation);

	Packet hello;
	host.Receive(hello, 9);
	PoolDescriptor parsed;
	Check(ValidateHello(hello.json, hello.fds.size(), parsed), "valid v2 pool hello");
	Check(hello.json["frames"] == true && parsed.epoch == 3, "pool hello carries frames and epoch");
	host.Send({{"type", "refused"}, {"version", 2}, {"reason", "local"}});
	host.ExpectFramelessHello();
	host.AcceptFrameless(transport);
	Check(!ring->TryAcquire(), "refused pool leases are retired");
	close(fd);
}

void TestUnknownAndSessionState() {
	Host host;
	Transport transport(host.path, {}, {}, [](const Json& apply) { return Applied(apply["id"]); }, true);
	host.ConnectFrameless(transport);
	host.Send({{"type", "session.state"}, {"state", "paused"}});
	host.Send({{"type", "x.y"}});
	host.Send(Apply(7, Json::object()));
	host.Expect(Applied(7));
}

void TestMetrics() {
	MetricsSubscription subscription;
	const Json first = {{"type", "metrics.subscribe"}, {"keys", {"game_fps", "nope"}},
	                    {"interval_ms", 10}};
	const Json second = {{"type", "metrics.subscribe"}, {"keys", {"frame_time_ms"}},
	                     {"interval_ms", 1000}};
	Check(ParseMetricsSubscribe(first, subscription), "parse metrics subscription");
	Check(subscription.interval_ms == 1000, "metrics interval clamps to 1000");
	MetricValues values;
	values.Set(Metric::GameFps, 50);
	values.Set(Metric::FrameTimeMs, 20);
	const Json fps = {{"type", "metrics"}, {"t_ms", 123}, {"values", {{"game_fps", 50}}}};
	Check(BuildMetricsMessage(subscription.keys, values, 123) == fps, "only game_fps subscribed");
	Check(ParseMetricsSubscribe(second, subscription), "replace metrics subscription");
	const Json frame_time = {{"type", "metrics"}, {"t_ms", 124}, {"values", {{"frame_time_ms", 20}}}};
	Check(BuildMetricsMessage(subscription.keys, values, 124) == frame_time,
	      "only frame_time_ms subscribed");

	Host host;
	Transport transport(host.path, {}, {{}, [values](MetricMask) { return values; }}, {}, true);
	host.ConnectFrameless(transport);
	host.Send(first);
	{
		Packet packet;
		host.Receive(packet);
		Check(packet.json["type"] == "metrics" && packet.json["values"] == fps["values"],
		      "first subscription applies on the connection");
	}
	host.Send(second);
	Packet packet;
	host.Receive(packet);
	Check(packet.json["type"] == "metrics" && packet.json["values"] == frame_time["values"],
	      "second subscription replaces the first on the connection");
}
} // namespace

int main() {
	TestEnvironment();
	TestMetricsOnlyHandshake();
	TestWelcomeVersion1Rejected();
	TestAcceptedVersion1Rejected();
	TestSettingsApplyRoundTrip();
	TestSettingsApplyBeforeAccepted();
	TestOutputAfterResolution();
	TestLocalRefusedFallsBackToFrameless();
	TestUnknownAndSessionState();
	TestMetrics();
	std::puts("session socket tests: ok");
}
