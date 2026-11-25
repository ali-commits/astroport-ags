#include "graphics/host_gpu/renderer/upscalePass.h"

#include "common/logging/log.h"
#include "graphics/host_gpu/renderer/cache/bufferCache.h"
#include "graphics/host_gpu/renderer/cache/textureCache.h"
#include "graphics/host_gpu/renderer/colorRenderTarget.h"
#include "graphics/host_gpu/renderer/depthRenderTarget.h"
#include "graphics/host_gpu/renderer/dlss.h"
#include "graphics/host_gpu/renderer/image/image.h"
#include "graphics/host_gpu/renderer/pipeline/descriptors.h"
#include "graphics/host_gpu/renderer/render.h"
#include "graphics/host_gpu/renderer/renderContext.h"
#include "graphics/host_gpu/vulkanCommon.h"
#include "graphics/shader/shader.h"
#include "graphics/shader/shaderBindings.h"
#include "kernel/memory.h"

#include <algorithm>
#include <array>
#include <chrono>
#include <fmt/format.h>
#include <optional>
#include <string>
#include <utility>

namespace Libs::Graphics {
namespace {

using UpscalePass::Outcome;

struct DepthUse {
	ImageId                      id;
	uint32_t                     layer = 0;
	std::optional<vk::CompareOp> compare; // of the last draw that tested depth
	std::optional<float>         clear;   // of the last draw that cleared depth
};

// Read and written under the render lock (draws).
struct PassState {
	UpscalePass::Layout              layout = UpscalePass::GetLayout();
	UpscalePass::DepthRing<DepthUse> ring;
	UpscalePass::Stats               stats;
	std::optional<uint32_t>          last_frame; // of the last frame DLSS upscaled
	uint64_t                         sizes     = 0; // the input and output size `refused` is about
	bool                             refused   = false;
	bool                             described = false;
	std::chrono::steady_clock::time_point reported = std::chrono::steady_clock::now();
};

PassState& State() {
	static PassState state;
	return state;
}

void Say(const std::string& text) {
	Log::WriteToConsoleAndLog("DLSS pass: " + text + "\n");
}

std::string DescribeImage(const Image* image) {
	if (image == nullptr) {
		return "no image";
	}
	return fmt::format("0x{:x} {} {}x{} samples {} storage {}", image->info.data.address,
	                   vk::to_string(image->backing.format), image->backing.extent.width,
	                   image->backing.extent.height, image->backing.samples,
	                   image->backing.usage & vk::ImageUsageFlagBits::eStorage ? 1 : 0);
}

// The facts a layout is written from and DLSS depends on, once.
void DescribePass(TextureCache& cache, const PassState& state, const ShaderStageRuntime& pixel,
                  const PreparedBindings& bindings, std::span<const RenderColorInfo> colors) {
	const auto& options = Embed::GetOptions();
	Say(fmt::format("shader {:016x} drawn; NGX {}; layout {}; reverse depth configured {}",
	                options.upscale_pass_hash, Dlss::Capabilities() != nullptr ? "on" : "off",
	                state.layout.given != 0 ? "given" : "not given",
	                options.dlss_reverse_depth ? 1 : 0));
	const auto& buffers = pixel.resources->buffers;
	for (size_t i = 0; i < buffers.size() && i < 4; i++) {
		if (buffers[i].dword_count < 4) {
			continue;
		}
		const auto descriptor = DecodeNativeDescriptor<ShaderBufferResource>(buffers[i]);
		std::array<uint32_t, 16> words {};
		const auto bytes = std::min<uint64_t>(descriptor.GetSize(), sizeof(words)) & ~uint64_t {3};
		auto text = fmt::format("buffer {} 0x{:x} {} bytes:", i, descriptor.Base48(),
		                        descriptor.GetSize());
		if (bytes != 0 && LibKernel::Memory::TryReadBacking(descriptor.Base48(), words.data(), bytes)) {
			for (size_t word = 0; word < bytes / 4; word++) {
				text += fmt::format(" {:08x}", words[word]);
			}
		} else {
			text += " unreadable";
		}
		Say(text);
	}
	for (size_t i = 0; i < bindings.images.size(); i++) {
		const auto& binding = bindings.images[i];
		auto        text = fmt::format("image {} {}", i, DescribeImage(cache.TryGetImage(binding.image_id)));
		if (const auto* depth = state.ring.Find(binding.desc.info.data.address); depth != nullptr) {
			text += fmt::format("; drawn with depth {}, compare {}, clear {}",
			                    DescribeImage(cache.TryGetImage(depth->id)),
			                    depth->compare ? vk::to_string(*depth->compare) : "not seen",
			                    depth->clear ? fmt::format("{}", *depth->clear) : "not seen");
		}
		Say(text);
	}
	for (size_t i = 0; i < colors.size(); i++) {
		Say(fmt::format("target {} {} view {} export mapping 0x{:02x}", i,
		                DescribeImage(cache.TryGetImage(colors[i].image_id)),
		                vk::to_string(colors[i].desc.view_info.format), colors[i].export_mapping.packed));
	}
}

} // namespace

void UpscalePass::NoteTargets(std::span<const RenderColorInfo> colors, const RenderDepthInfo& depth) {
	if (!depth.image_id) {
		return;
	}
	auto& ring = State().ring;
	for (const auto& color: colors) {
		const auto address = color.desc.info.data.address;
		DepthUse   use {depth.image_id, depth.desc.view_info.base_layer, std::nullopt, std::nullopt};
		if (const auto* last = ring.Find(address); last != nullptr && last->id == use.id) {
			use.compare = last->compare;
			use.clear   = last->clear;
		}
		if (depth.depth_test_enable) {
			use.compare = depth.depth_compare_op;
		}
		if (depth.depth_clear_enable) {
			use.clear = depth.depth_clear_value;
		}
		ring.Note(address, use);
	}
}

bool RenderExecutor::UpscalePassDraw(CommandBuffer& buffer, const ShaderStageRuntime& pixel,
                                     const PreparedBindings&          bindings,
                                     std::span<const RenderColorInfo> colors, bool continued) {
	auto&       state   = State();
	auto&       cache   = m_context.GetTextureCache();
	const auto& layout  = state.layout;
	const auto& options = Embed::GetOptions();
	const auto  outcome = [&]() -> Outcome {
		if (!std::exchange(state.described, true)) {
			DescribePass(cache, state, pixel, bindings, colors);
		}
		if (continued) {
			return Outcome::Continued;
		}
		if (layout.given == 0) {
			return Outcome::Constants;
		}
		const auto& buffers = pixel.resources->buffers;
		if (layout.constants >= buffers.size() || buffers[layout.constants].dword_count < 4 ||
		    layout.motion_image >= bindings.images.size() ||
		    layout.color_image >= bindings.images.size() || colors.empty()) {
			return Outcome::Bindings;
		}
		const auto descriptor = DecodeNativeDescriptor<ShaderBufferResource>(buffers[layout.constants]);
		std::array<uint32_t, 256> words; // a layout index is a byte
		const auto                bytes = uint64_t {UpscalePass::WordCount(layout)} * 4;
		if (descriptor.GetSize() < bytes) {
			return Outcome::Bindings;
		}
		// Constants the GPU wrote are not in guest memory yet.
		if (m_context.GetBufferCache().HasGpuDirtyBytes(descriptor.Base48(), bytes) ||
		    !LibKernel::Memory::TryReadBacking(descriptor.Base48(), words.data(), bytes)) {
			return Outcome::Constants;
		}
		const auto constants = UpscalePass::Read(layout, std::span(words.data(), bytes / 4));
		if (!constants) {
			return Outcome::Constants;
		}
		const auto& input  = constants->input;
		const auto& output = constants->output;
		const auto& motion = bindings.images[layout.motion_image];
		const auto& color  = bindings.images[layout.color_image];
		const auto& target = colors[0];
		if (!constants->Valid() || target.Extent() != vk::Extent2D {output[0], output[1]}) {
			return Outcome::Sizes;
		}
		if (!constants->motion_used) {
			return Outcome::NoMotion;
		}
		// The depth target of the draws that wrote the motion vectors.
		const auto* depth_use   = state.ring.Find(motion.desc.info.data.address);
		const auto* depth_image = depth_use != nullptr ? cache.TryGetImage(depth_use->id) : nullptr;
		if (depth_image == nullptr || !depth_image->registered || depth_image->binding.needs_rebind ||
		    depth_image->depth_id || !depth_image->info.IsDepth() ||
		    depth_use->layer >= depth_image->backing.layers) {
			return Outcome::NoDepth;
		}
		const auto* motion_image = cache.TryGetImage(motion.image_id);
		const auto* color_image  = cache.TryGetImage(color.image_id);
		const auto* output_image = cache.TryGetImage(target.image_id);
		const auto  usable = [](const Image* image, uint32_t level, const uint32_t* size) {
			return image != nullptr && image->backing.image != nullptr && image->backing.samples == 1 &&
			       image->backing.image_type == vk::ImageType::e2D &&
			       level < image->backing.mip_levels &&
			       image->backing.extent.width >> level >= size[0] &&
			       image->backing.extent.height >> level >= size[1];
		};
		const auto& target_view = target.desc.view_info;
		if (!usable(motion_image, motion.desc.view_info.base_level, input) ||
		    !usable(color_image, color.desc.view_info.base_level, input) ||
		    !usable(depth_image, 0, input) || !usable(output_image, target_view.base_level, output) ||
		    !(depth_image->backing.usage & vk::ImageUsageFlagBits::eSampled) ||
		    motion.image_view == nullptr || color.image_view == nullptr ||
		    motion_image->info.IsDepth() || color_image->info.IsDepth() ||
		    output_image->info.IsDepth() || output_image == motion_image ||
		    output_image == color_image || !output_image->registered ||
		    output_image->binding.needs_rebind || output_image->depth_id ||
		    !(output_image->backing.usage & vk::ImageUsageFlagBits::eStorage) ||
		    // DLSS writes the image's own components through a storage view, and Image::FindView
		    // stops the emulator for a view the image cannot have.
		    target_view.format != output_image->backing.format ||
		    target.export_mapping.packed != Prospero::ColorComponentMapping::kIdentity ||
		    target_view.type != vk::ImageViewType::e2D || target_view.level_count != 1 ||
		    target_view.layer_count != 1) {
			return Outcome::Images;
		}
		if (Dlss::Capabilities() == nullptr) {
			return Outcome::Off;
		}
		const auto request = UpscalePass::MakeRequest(*constants, options.dlss_reverse_depth,
		                                              state.last_frame, options.upscale_pass_hash);
		const auto sizes   = uint64_t {input[0]} | uint64_t {input[1]} << 16u |
		                     uint64_t {output[0]} << 32u | uint64_t {output[1]} << 48u;
		if (sizes != state.sizes) {
			Dlss::Inputs probe {};
			probe.output.width  = output[0];
			probe.output.height = output[1];
			const auto creation = Dlss::Choose(*Dlss::Capabilities(), request, probe);
			state.sizes         = sizes;
			state.refused       = !creation;
			Say(fmt::format(
			    "{}x{} -> {}x{}: {}; modes at this output: {}", input[0], input[1], output[0], output[1],
			    creation ? fmt::format("quality mode {}, render sizes {}x{}..{}x{}", creation->mode,
			                           creation->min_width, creation->min_height, creation->max_width,
			                           creation->max_height)
			             : std::string("no quality mode takes this render size"),
			    Dlss::DescribeModes(*Dlss::Capabilities(), output[0], output[1])));
		}
		if (state.refused) {
			return Outcome::Refused;
		}
		// From here the cache and the command buffer change; the title's pass can still run after it.
		TextureCache::ImageDesc depth_desc;
		depth_desc.info                 = depth_image->info;
		depth_desc.view_info.format     = depth_image->backing.format;
		depth_desc.view_info.aspect     = vk::ImageAspectFlagBits::eDepth;
		depth_desc.view_info.base_layer = depth_use->layer;
		const UpscaleImage depth_input {depth_use->id, cache.FindTexture(depth_use->id, depth_desc),
		                                depth_image->backing.format, 0, depth_use->layer};
		// As the pass's own draw would: the write claim, the refresh and the readback tracking.
		(void)cache.FindRenderTarget(target.image_id, target.desc);
		auto storage_view  = target_view;
		storage_view.usage = vk::ImageUsageFlagBits::eStorage;
		const UpscaleImage upscaled {target.image_id, cache.GetImage(target.image_id).FindView(storage_view),
		                             target_view.format, target_view.base_level, target_view.base_layer};
		const auto         sampled = [](const TextureBinding& binding) {
			const auto& view = binding.desc.view_info;
			return UpscaleImage {binding.image_id, binding.image_view, view.format, view.base_level,
			                     view.base_layer};
		};
		if (!UpscaleResolved(buffer, request, sampled(color), depth_input, sampled(motion), upscaled,
		                     {output[0], output[1]})) {
			state.refused = true; // until the sizes change: creation or evaluation failed
			return Outcome::Refused;
		}
		cache.GetImage(target.image_id).NoteContentWrite();
		state.last_frame = constants->frame;
		return Outcome::Evaluated;
	}();
	state.stats.Add(outcome);
	if (outcome != Outcome::Evaluated) {
		state.last_frame.reset();
	}
	if (const auto now = std::chrono::steady_clock::now(); now - state.reported >= std::chrono::seconds(10)) {
		state.reported = now;
		if (const auto text = state.stats.Take(); !text.empty()) {
			Log::WriteToConsoleAndLog(text + "\n");
		}
	}
	return outcome == Outcome::Evaluated;
}

} // namespace Libs::Graphics
