// Kyty's renderer inside a host process (libkyty_renderer.so): what Emulator::Run does, without
// loading a guest executable. Only the kyty_renderer target compiles this file.

#include "embed/kytyEmbed.h"

#include "common/emulatorConfig.h"
#include "common/file.h"
#include "common/logging/log.h"
#include "common/profiler.h"
#include "common/subsystems.h"
#include "common/threads.h"
#include "embed/embedOptions.h"
#include "graphics/host_gpu/faultCost.h"
#include "graphics/host_gpu/pageManager.h"
#include "graphics/host_gpu/renderer/pipeline/pipelineList.h"
#include "graphics/presentation/videoOut.h"
#include "graphics/presentation/window.h"
#include "kernel/eventQueue.h"
#include "kernel/memory.h"
#include "kernel/pthread.h"
#include "libs/agc.h"
#include "libs/controller.h"
#include "libs/dualSenseHaptics.h"
#include "libs/libs.h"
#include "libs/padData.h"
#include "loader/symbolDatabase.h"
#include "loader/systemContent.h"
#include "streaming/metricsSource.h"
#include "streaming/sessionEnv.h"
#include "streaming/transport.h"

#include <SDL3/SDL.h>
#include <atomic>
#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <mutex>
#include <string>
#include <thread>

// No header declares these (as in libs/libs.cpp).
namespace Libs {
LIB_DEFINE(InitAgcDriver_1);
LIB_DEFINE(InitLibKernel_1_Mem);
namespace LibGen5 {
LIB_DEFINE(InitVideoOut_1);
} // namespace LibGen5
} // namespace Libs

namespace {

namespace Haptics = Libs::Controller::DualSenseHaptics;

Loader::SymbolDatabase g_symbols;

struct PadAudioTarget {
	Haptics::Stream* stream = nullptr;
	uint32_t         rate   = 0;
};
std::mutex     g_pad_audio_mutex;
PadAudioTarget g_pad_audio[2]; // vibration, speaker
bool           g_flushed = false;
std::atomic<bool> g_ready {false};     // set just before hooks->ready()
int64_t           g_amm_wait_ms = 5000; // KYTY_FAULT_AMM_WAIT_MS, read in Run (not in the fault path)

const char* Text(const char* text) {
	return text != nullptr ? text : "";
}

int Run(const KytyEmbedConfig* config, const KytyEmbedHooks* hooks) {
	if (config == nullptr || hooks == nullptr || config->size < sizeof(KytyEmbedConfig) ||
	    hooks->size < sizeof(KytyEmbedHooks) || config->app0_dir == nullptr) {
		return 1;
	}
	static std::atomic_flag started;
	if (started.test_and_set()) {
		return 1; // one renderer per process
	}
	if (const char* value = std::getenv("KYTY_FAULT_AMM_WAIT_MS"); value != nullptr && value[0] != '\0') {
		g_amm_wait_ms = std::strtoll(value, nullptr, 10);
	}
	// The calling thread is Kyty's main thread from here on.
	Common::InitializeThreads();

	Config::ConfigOptions cfg;
	if (config->width != 0) cfg.screen_width = config->width;
	if (config->height != 0) cfg.screen_height = config->height;
	cfg.fullscreen_enabled             = config->fullscreen != 0;
	cfg.hide_cursor_enabled            = config->fullscreen != 0;
	cfg.present_mode                   = config->present_mode == 0   ? Config::PresentMode::Immediate
	                                     : config->present_mode == 1 ? Config::PresentMode::Fifo
	                                                                 : Config::PresentMode::Mailbox;
	cfg.shader_optimization_type       = config->shader_optimization == 0 ? Config::ShaderOptimizationType::None
	                                     : config->shader_optimization == 1
	                                         ? Config::ShaderOptimizationType::Size
	                                         : Config::ShaderOptimizationType::Performance;
	cfg.vblank_frequency               = config->vblank_hz != 0 ? config->vblank_hz : 60;
	cfg.controller_vibration_intensity = config->vibration_intensity;
	cfg.dlss_folder                    = Text(config->dlss_dir);
	cfg.dlss_preset                    = config->dlss_preset;
	cfg.splash_enabled                 = false;

	auto& options               = Embed::GetOptions();
	options.window_title        = Text(config->window_title);
	options.ngx_data_dir        = Text(config->ngx_data_dir);
	options.fsr_dir             = Text(config->fsr_dir);
	options.upscale_pass_hash   = config->upscale_pass_hash;
	options.rumble_scale        = config->rumble_scale;
	options.rumble_switch_floor = config->rumble_switch_floor;
	options.pad_prefer          = config->pad_prefer;
	options.wayland_native      = config->wayland_native != 0;
	options.low_latency         = config->low_latency != 0;
	options.dlss                = config->dlss != 0;
	options.dlss_reverse_depth  = config->dlss_reverse_depth != 0;
	options.ds_rumble           = config->ds_rumble != 0;
	options.fsr_fg              = config->fsr_fg != 0;
	options.low_latency_interval_us.store(config->low_latency_interval_us);
	options.present_gate = hooks->present_gate;
	options.presented    = hooks->presented;

	const std::filesystem::path app0 = config->app0_dir;
	Libs::Graphics::SetPipelineListExecutable(app0 / "eboot.bin");

	// Never destroyed: guest threads of the host keep running; flush() saves what must persist.
	auto& subsystems = *new Common::Subsystems(true);
	subsystems.Initialize<Config::Lifecycle>();
	Config::Load(cfg);
	subsystems.Initialize<Log::Lifecycle>();
	if (const auto param_json = app0 / "sce_sys" / "param.json"; Common::File::IsFileExisting(param_json)) {
		Loader::SystemContentLoadParamSfo(param_json);
	}
	subsystems.Initialize<Libs::LibKernel::PthreadLifecycle>();
	subsystems.Initialize<Profiler::Lifecycle>();
	subsystems.Initialize<Libs::LibKernel::Memory::Lifecycle>();
	if (hooks->reserved != nullptr) hooks->reserved();
	subsystems.Initialize<Libs::Controller::Lifecycle>();
	subsystems.Initialize<Libs::Graphics::Lifecycle>();

	Libs::InitAgcDriver_1(&g_symbols);
	Libs::LibGen5::InitVideoOut_1(&g_symbols);
	Libs::InitLibKernel_1_Mem(&g_symbols);

	if (const char* socket = Streaming::SessionSocket(); socket != nullptr) {
		// Without a host hook the transport skips apply packets; a reply that is not a JSON object
		// becomes an empty one, because the transport drops the connection on anything else.
		const auto live = hooks->live_setting;
		using Hook      = Streaming::Transport::SettingsHook;
		new Streaming::Transport(socket, {}, Streaming::MakeEmulatorMetricsHooks(),
		                         live == nullptr ? Hook {} : Hook([live](const Streaming::Json& apply) {
			                         const char* reply = live(apply.dump().c_str());
			                         auto json = reply != nullptr ? Streaming::Json::parse(reply, nullptr, false)
			                                                      : Streaming::Json();
			                         return json.is_object() ? json : Streaming::Json::object();
		                         }),
		                         true);
	}

	g_ready.store(true);
	if (hooks->ready != nullptr) hooks->ready();
	Libs::Graphics::WindowRun();
	return 0;
}

void* Find(const char* nid) {
	const auto* record = nid != nullptr ? g_symbols.FindByNid(nid, Loader::SymbolType::Func) : nullptr;
	return record != nullptr ? reinterpret_cast<void*>(record->vaddr) : nullptr;
}

// The live part of TryHandleGuestAccessFault (loader/runtimeLinker.cpp).
int Fault(uint64_t address, int access, uint64_t instruction) {
	namespace Memory = Libs::LibKernel::Memory;
	if (access < 0 || access > 2) {
		return 0;
	}
	if (Libs::Graphics::FaultCost::MapEnabled() || Libs::Graphics::FaultCost::AttribEnabled()) {
		Libs::Graphics::FaultCost::SetFaultInstruction(instruction);
	}
	// The host retries the faulting instruction when this returns non-zero.
	const Libs::Graphics::FaultCost::RetriedFaultScope retried;
	if (Memory::HandleGpuFault(static_cast<Libs::Graphics::PageFaultAccess>(access), address)) {
		return 1;
	}
	// KYTY_FAULT_AMM_WAIT_MS: memory a queued AMM map will map; wait for the map, then retry.
	if (access == 2 || !Memory::PendingAmmMapCovers(address)) {
		return 0;
	}
	const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(g_amm_wait_ms);
	while (std::chrono::steady_clock::now() < deadline && Memory::PendingAmmMapCovers(address)) {
		std::this_thread::sleep_for(std::chrono::microseconds(200));
	}
	return Memory::PendingAmmMapCovers(address) ? 0 : 1;
}

void Quit() {
	SDL_Event event {};
	event.type = SDL_EVENT_QUIT;
	SDL_PushEvent(&event);
}

void Flush() {
	{
		std::lock_guard lock(g_pad_audio_mutex);
		g_flushed = true;
		for (auto& target: g_pad_audio) {
			Haptics::Close(target.stream);
			target.stream = nullptr;
		}
	}
	// Before ready, run may still be starting subsystems on its thread, and nothing is worth saving.
	if (g_ready.load()) {
		Common::Subsystems::EmergencyShutdownActive();
	}
}

int UserId() {
	return Config::GetUserId();
}

// shortcut: only the active controller is used, no SDL enumeration; upgrade if a pad can be
// connected without becoming Kyty's active controller.
uint64_t PadAudio(uint32_t rate, int speaker, const void* samples, uint32_t frames, uint32_t channels,
                  int is_float, float gain) {
	static constexpr int unity[8] = {32768, 32768, 32768, 32768, 32768, 32768, 32768, 32768};
	if (channels == 0 || channels > 8) {
		return 0;
	}
	std::lock_guard lock(g_pad_audio_mutex);
	if (g_flushed) {
		return 0;
	}
	auto& target = g_pad_audio[speaker != 0 ? 1 : 0];
	if (target.stream == nullptr || target.rate != rate) {
		Haptics::Close(target.stream);
		target.stream = Haptics::Open(rate, speaker != 0);
		target.rate   = rate;
	}
	using Libs::Controller::Setting;
	return Haptics::Queue(target.stream, Libs::Controller::GetActiveControllerId(), samples, frames, channels,
	                      is_float != 0, unity,
	                      gain * Libs::Controller::GetSettingScale(speaker != 0 ? Setting::SpeakerVolume
	                                                                            : Setting::VibrationIntensity));
}

void SetLowLatencyInterval(uint32_t us) {
	Embed::GetOptions().low_latency_interval_us.store(us, std::memory_order_relaxed);
}

void SetWindowSize(uint32_t width, uint32_t height) {
	Embed::GetOptions().pending_window_size.store((static_cast<uint64_t>(width) << 32) | height,
	                                              std::memory_order_relaxed);
}

template <typename Function>
void* Address(Function function) {
	return reinterpret_cast<void*>(function);
}

} // namespace

extern "C" [[gnu::visibility("default")]] const KytyEmbedApi* kyty_embed_api(void) {
	namespace Pad = Libs::Controller;
	namespace Eq  = Libs::LibKernel::EventQueue;
	static const KytyEmbedFns fns = {
	    Address(Pad::PadInit),
	    Address(Pad::PadOpen),
	    Address(Pad::PadGetHandle),
	    Address(Pad::PadReadState),
	    Address(Pad::PadSetVibration),
	    Address(Pad::PadSetLightBar),
	    Address(Pad::PadResetLightBar),
	    Address(Pad::PadSetTriggerEffect),
	    Address(Eq::KernelCreateEqueue),
	    Address(Eq::KernelWaitEqueue),
	    Address(Eq::KernelAddUserEvent),
	    Address(Eq::KernelTriggerUserEvent),
	    Address(Libs::Graphics::Gen5Driver::AgcDriverAddEqEvent),
	    Address(Libs::VideoOut::VideoOutAddFlipEvent),
	};
	static const KytyEmbedApi api = {
	    KYTY_EMBED_ABI, sizeof(KytyEmbedApi), sizeof(Libs::Controller::PadData),
	    &fns,           Run,                  Find,
	    Fault,          Quit,                 Flush,
	    UserId,         PadAudio,             SetLowLatencyInterval,
	    SetWindowSize,
	};
	return &api;
}
