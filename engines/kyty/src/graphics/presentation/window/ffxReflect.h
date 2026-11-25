#ifndef EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXREFLECT_H_
#define EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXREFLECT_H_

// The descriptor bindings of a SPIR-V module, as the FidelityFX backend wants them listed for a
// shader (ffxBlobs.cpp): by kind, each with the variable's name, binding, set and array length.
// Plain SPIR-V reading, no SDK and no Vulkan, so it is tested alone (tests/FrameGenTests.cpp).

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <unordered_map>
#include <vector>

namespace Libs::Graphics::FfxReflect {

enum class Kind : uint8_t {
	Sampler,
	Texture,        // sampled image
	StorageTexture, // storage image
	Constants,      // uniform block
	Buffer,         // storage buffer no member of which is written
	StorageBuffer,
	Count
};

struct Binding {
	std::string name; // of the variable
	uint32_t    binding = 0;
	uint32_t    set     = 0;
	uint32_t    count   = 1; // array length
};

struct Reflection {
	std::array<std::vector<Binding>, static_cast<size_t>(Kind::Count)> kinds;
	// The module needs shaderStorageImageReadWithoutFormat / ...WriteWithoutFormat.
	bool read_without_format  = false;
	bool write_without_format = false;

	[[nodiscard]] const std::vector<Binding>& Of(Kind kind) const {
		return kinds[static_cast<size_t>(kind)];
	}
};

// False for anything that is not a complete SPIR-V module, or a bound variable of a type that is
// none of the kinds; `out` is then unspecified.
[[nodiscard]] inline bool Reflect(std::span<const uint32_t> words, Reflection& out) {
	enum : uint32_t {
		OpName = 5, OpCapability = 17, OpTypeImage = 25, OpTypeSampler = 26, OpTypeSampledImage = 27,
		OpTypeArray = 28, OpTypeRuntimeArray = 29, OpTypeStruct = 30, OpTypePointer = 32,
		OpConstant = 43, OpVariable = 59, OpDecorate = 71, OpMemberDecorate = 72,
		DecorationBlock = 2, DecorationBufferBlock = 3, DecorationNonWritable = 24,
		DecorationBinding = 33, DecorationDescriptorSet = 34,
		StorageUniform = 2, StorageStorageBuffer = 12,
		CapabilityReadWithoutFormat = 55, CapabilityWriteWithoutFormat = 56,
	};
	struct Type {
		uint32_t op      = 0;
		uint32_t inner   = 0; // pointee or element type
		uint32_t length  = 0; // array: the id of the length constant
		uint32_t sampled = 0; // image
		uint32_t members = 0; // struct
	};
	struct Decorations {
		bool     block = false, buffer_block = false, non_writable = false;
		bool     has_binding = false;
		uint32_t binding = 0, set = 0;
		uint32_t read_only_members = 0;
	};
	struct Variable {
		uint32_t id, type, storage;
	};
	out = {};
	if (words.size() < 5 || words[0] != 0x07230203u) {
		return false;
	}
	std::unordered_map<uint32_t, Type>        types;
	std::unordered_map<uint32_t, Decorations> decorations;
	std::unordered_map<uint32_t, std::string> names;
	std::unordered_map<uint32_t, uint32_t>    constants;
	std::vector<Variable>                     variables;
	for (size_t at = 5; at < words.size();) {
		const uint32_t count = words[at] >> 16u;
		const uint32_t op    = words[at] & 0xffffu;
		if (count == 0 || count > words.size() - at) {
			return false;
		}
		const auto* w = words.data() + at;
		at += count;
		switch (op) {
			case OpCapability:
				if (count >= 2) {
					out.read_without_format |= w[1] == CapabilityReadWithoutFormat;
					out.write_without_format |= w[1] == CapabilityWriteWithoutFormat;
				}
				break;
			case OpName:
				if (count >= 3) {
					std::string name;
					for (uint32_t i = 2; i < count; i++) {
						for (uint32_t byte = 0; byte < 4; byte++) {
							const auto c = static_cast<char>(w[i] >> (8u * byte));
							if (c == 0) {
								i = count;
								break;
							}
							name += c;
						}
					}
					names[w[1]] = std::move(name);
				}
				break;
			case OpDecorate:
				if (count >= 3) {
					auto& d = decorations[w[1]];
					d.block |= w[2] == DecorationBlock;
					d.buffer_block |= w[2] == DecorationBufferBlock;
					d.non_writable |= w[2] == DecorationNonWritable;
					if (w[2] == DecorationBinding && count >= 4) {
						d.has_binding = true;
						d.binding     = w[3];
					}
					if (w[2] == DecorationDescriptorSet && count >= 4) {
						d.set = w[3];
					}
				}
				break;
			case OpMemberDecorate:
				// glslang writes NonWritable once per member of a readonly block.
				if (count >= 4 && w[3] == DecorationNonWritable) {
					decorations[w[1]].read_only_members++;
				}
				break;
			case OpTypeImage:
				if (count >= 8) {
					types[w[1]] = {op, 0, 0, w[7], 0};
				}
				break;
			case OpTypeSampler:
			case OpTypeSampledImage:
				if (count >= 2) {
					types[w[1]] = {op, 0, 0, 0, 0};
				}
				break;
			case OpTypeArray:
				if (count >= 4) {
					types[w[1]] = {op, w[2], w[3], 0, 0};
				}
				break;
			case OpTypeRuntimeArray:
				if (count >= 3) {
					types[w[1]] = {op, w[2], 0, 0, 0};
				}
				break;
			case OpTypeStruct:
				if (count >= 2) {
					types[w[1]] = {op, 0, 0, 0, count - 2};
				}
				break;
			case OpTypePointer:
				if (count >= 4) {
					types[w[1]] = {op, w[3], 0, 0, 0};
				}
				break;
			case OpConstant:
				if (count >= 4) {
					constants[w[2]] = w[3];
				}
				break;
			case OpVariable:
				if (count >= 4) {
					variables.push_back({w[2], w[1], w[3]});
				}
				break;
			default: break;
		}
	}
	for (const auto& variable: variables) {
		const auto decoration = decorations.find(variable.id);
		if (decoration == decorations.end() || !decoration->second.has_binding) {
			continue;
		}
		const auto pointer = types.find(variable.type);
		if (pointer == types.end() || pointer->second.op != OpTypePointer) {
			return false;
		}
		Binding binding {names[variable.id], decoration->second.binding, decoration->second.set, 1};
		auto    type_id = pointer->second.inner;
		auto    type    = types.find(type_id);
		if (type != types.end() && type->second.op == OpTypeArray) {
			const auto length = constants.find(type->second.length);
			if (length == constants.end()) {
				return false;
			}
			binding.count = length->second;
			type_id       = type->second.inner;
			type          = types.find(type_id);
		}
		if (type == types.end()) {
			return false;
		}
		Kind kind;
		switch (type->second.op) {
			case OpTypeSampler: kind = Kind::Sampler; break;
			case OpTypeSampledImage: kind = Kind::Texture; break;
			case OpTypeImage:
				kind = type->second.sampled == 2 ? Kind::StorageTexture : Kind::Texture;
				break;
			case OpTypeStruct: {
				const auto& block = decorations[type_id];
				if (variable.storage == StorageUniform && block.block) {
					kind = Kind::Constants;
				} else if (variable.storage == StorageStorageBuffer || block.buffer_block) {
					const bool read_only =
					    decoration->second.non_writable ||
					    (type->second.members != 0 && block.read_only_members >= type->second.members);
					kind = read_only ? Kind::Buffer : Kind::StorageBuffer;
				} else {
					return false;
				}
				break;
			}
			default: return false;
		}
		out.kinds[static_cast<size_t>(kind)].push_back(std::move(binding));
	}
	return true;
}

} // namespace Libs::Graphics::FfxReflect

#endif // EMULATOR_SRC_GRAPHICS_PRESENTATION_WINDOW_FFXREFLECT_H_
