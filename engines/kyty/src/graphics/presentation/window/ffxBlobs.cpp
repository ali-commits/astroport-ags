#include "graphics/presentation/window/ffxBlobs.h"

#include "graphics/presentation/window/ffxReflect.h"

#include <FidelityFX/host/ffx_frameinterpolation.h>
#include <FidelityFX/host/ffx_opticalflow.h>
#include <array>
#include <cstring>
#include <ffx_blob_table.h> // generated: the modules and KYTY_FFX_BLOBS
#include <ffx_shader_blobs.h>
#include <frameinterpolation/ffx_frameinterpolation_private.h>
#include <opticalflow/ffx_opticalflow_private.h>
#include <vector>

namespace Libs::Graphics::FfxBlobs {
namespace {

using FfxReflect::Kind;

constexpr Row ROWS[] = {
#define KYTY_FFX_BLOB(effect, pass, key, symbol) {effect, pass, key, symbol},
    KYTY_FFX_BLOBS(KYTY_FFX_BLOB)
#undef KYTY_FFX_BLOB
};

// One kind's bindings in the parallel arrays FfxShaderBlob points to.
struct List {
	std::vector<const char*> names;
	std::vector<uint32_t>    bindings, counts, sets;
};

struct Entry {
	FfxReflect::Reflection                               reflection;
	std::array<List, static_cast<size_t>(Kind::Count)> lists;
	bool                                                 valid = false;
};

// Reflected once. The lists point into the entries' strings: the vector's elements stay where
// they are when it is returned.
std::vector<Entry>& Entries() {
	static std::vector<Entry> entries = [] {
		std::vector<Entry> all(std::size(ROWS));
		for (size_t i = 0; i < all.size(); i++) {
			auto& entry = all[i];
			entry.valid = FfxReflect::Reflect(ROWS[i].words, entry.reflection);
			for (size_t kind = 0; kind < entry.lists.size(); kind++) {
				for (const auto& binding: entry.reflection.kinds[kind]) {
					entry.lists[kind].names.push_back(binding.name.c_str());
					entry.lists[kind].bindings.push_back(binding.binding);
					entry.lists[kind].counts.push_back(binding.count);
					entry.lists[kind].sets.push_back(binding.set);
				}
			}
		}
		return all;
	}();
	return entries;
}

} // namespace

std::span<const Row> Rows() {
	return ROWS;
}

} // namespace Libs::Graphics::FfxBlobs

extern "C" {

FfxErrorCode ffxGetPermutationBlobByIndex(FfxEffect effectId, FfxPass passId, FfxBindStage /*bindStage*/,
                                          uint32_t permutationOptions, FfxShaderBlob* outBlob) {
	using namespace Libs::Graphics::FfxBlobs;
	// The options that select a module here; the rest (16-bit math, 64-wide waves, and the
	// motion-vector options this build fixes) have one variant.
	uint32_t key = 0;
	if (effectId == FFX_EFFECT_FRAMEINTERPOLATION) {
		key = (permutationOptions & FRAMEINTERPOLATION_SHADER_PERMUTATION_DEPTH_INVERTED) != 0 ? 1 : 0;
	} else if (effectId == FFX_EFFECT_OPTICALFLOW) {
		key = (permutationOptions & OPTICALFLOW_HDR_COLOR_INPUT) != 0 ? 1 : 0;
	}
	auto& entries = Entries();
	for (size_t i = 0; i < std::size(ROWS); i++) {
		const auto& row = ROWS[i];
		if (row.effect != static_cast<uint32_t>(effectId) || row.pass != static_cast<uint32_t>(passId) ||
		    row.key != key || !entries[i].valid) {
			continue;
		}
		auto&      lists = entries[i].lists;
		const auto list  = [&](Kind kind) -> List& { return lists[static_cast<size_t>(kind)]; };
		const auto count = [&](Kind kind) { return static_cast<uint32_t>(list(kind).names.size()); };
		auto& cb = list(Kind::Constants);
		auto& st = list(Kind::Texture);
		auto& ut = list(Kind::StorageTexture);
		auto& sb = list(Kind::Buffer);
		auto& ub = list(Kind::StorageBuffer);
		auto& sm = list(Kind::Sampler);
		const FfxShaderBlob blob = {
		    reinterpret_cast<const uint8_t*>(row.words.data()),
		    static_cast<uint32_t>(row.words.size_bytes()),
		    count(Kind::Constants),
		    count(Kind::Texture),
		    count(Kind::StorageTexture),
		    count(Kind::Buffer),
		    count(Kind::StorageBuffer),
		    count(Kind::Sampler),
		    0,
		    cb.names.data(), cb.bindings.data(), cb.counts.data(), cb.sets.data(),
		    st.names.data(), st.bindings.data(), st.counts.data(), st.sets.data(),
		    ut.names.data(), ut.bindings.data(), ut.counts.data(), ut.sets.data(),
		    sb.names.data(), sb.bindings.data(), sb.counts.data(), sb.sets.data(),
		    ub.names.data(), ub.bindings.data(), ub.counts.data(), ub.sets.data(),
		    sm.names.data(), sm.bindings.data(), sm.counts.data(), sm.sets.data(),
		    nullptr, nullptr, nullptr, nullptr};
		std::memcpy(static_cast<void*>(outBlob), &blob, sizeof(blob));
		return FFX_OK;
	}
	std::memset(static_cast<void*>(outBlob), 0, sizeof(*outBlob));
	return FFX_ERROR_INVALID_ARGUMENT;
}

// No module here is built for 64-wide waves.
FfxErrorCode ffxIsWave64(FfxEffect /*effectId*/, uint32_t /*permutationOptions*/, bool& isWave64) {
	isWave64 = false;
	return FFX_OK;
}

} // extern "C"
