#include "graphics/presentation/window/ffxReflect.h"
#include "graphics/presentation/window/frameGenLogic.h"

#if KYTY_FSR_FRAMEGEN
#include "graphics/presentation/window/ffxBlobs.h"

#include <FidelityFX/host/ffx_frameinterpolation.h>
#include <FidelityFX/host/ffx_opticalflow.h>
#include <ffx_shader_blobs.h>
#endif

#include <cstring>
#include <gtest/gtest.h>
#include <initializer_list>
#include <set>
#include <string>
#include <utility>
#include <vector>

namespace {

using namespace Libs::Graphics;
using FfxReflect::Kind;

// A SPIR-V module written by hand: only what the reader looks at.
class Module {
public:
	Module() { m_words = {0x07230203u, 0x00010500u, 0, 100, 0}; }
	void Op(uint32_t op, std::initializer_list<uint32_t> operands) {
		m_words.push_back(static_cast<uint32_t>((operands.size() + 1) << 16u) | op);
		m_words.insert(m_words.end(), operands);
	}
	void Name(uint32_t id, const std::string& name) {
		std::vector<uint32_t> text(name.size() / 4 + 1, 0);
		std::memcpy(text.data(), name.data(), name.size());
		m_words.push_back(static_cast<uint32_t>((text.size() + 2) << 16u) | 5u);
		m_words.push_back(id);
		m_words.insert(m_words.end(), text.begin(), text.end());
	}
	// A variable `id` of pointer type `id + 100` to `type`, bound at (0, binding).
	void Variable(uint32_t id, uint32_t type, uint32_t storage, uint32_t binding, const std::string& name) {
		Name(id, name);
		Op(71, {id, 33, binding});
		Op(71, {id, 34, 0});
		Op(32, {id + 100, storage, type});
		Op(59, {id + 100, id, storage});
	}
	std::vector<uint32_t> m_words;
};

Module MakeModule() {
	Module m;
	m.Op(17, {1});               // Shader
	m.Op(17, {55});              // StorageImageReadWithoutFormat
	m.Op(22, {1, 32});           // float
	m.Op(21, {2, 32, 0});        // uint
	m.Op(43, {2, 3, 3});         // uint 3
	m.Op(26, {10});              // sampler
	m.Op(25, {11, 1, 1, 0, 0, 0, 1, 0}); // sampled image
	m.Op(25, {12, 1, 1, 0, 0, 0, 2, 0}); // storage image
	m.Op(28, {13, 11, 3});       // sampled image [3]
	m.Op(30, {14, 1});           // uniform block
	m.Op(71, {14, 2});
	m.Op(30, {15, 2, 2});        // read-only storage block: both members NonWritable
	m.Op(71, {15, 2});
	m.Op(72, {15, 0, 24});
	m.Op(72, {15, 1, 24});
	m.Op(30, {16, 2, 2});        // written storage block: one member NonWritable
	m.Op(71, {16, 2});
	m.Op(72, {16, 0, 24});
	m.Variable(20, 10, 0, 1000, "s_LinearClamp");
	m.Variable(21, 11, 0, 0, "r_input_depth");
	m.Variable(22, 12, 0, 4, "rw_output");
	m.Variable(23, 13, 0, 7, "r_pyramid");
	m.Variable(24, 14, 2, 5, "cbFI");
	m.Variable(25, 15, 12, 8, "r_counters");
	m.Variable(26, 16, 12, 9, "rw_counters");
	// Not a resource: no binding.
	m.Name(30, "gl_GlobalInvocationID");
	m.Op(32, {130, 1, 2});
	m.Op(59, {130, 30, 1});
	return m;
}

TEST(FfxReflect, ListsEveryKindWithNameBindingAndCount) {
	const auto            module = MakeModule();
	FfxReflect::Reflection r;
	ASSERT_TRUE(FfxReflect::Reflect(module.m_words, r));
	const auto one = [&](Kind kind) -> const FfxReflect::Binding& {
		EXPECT_EQ(r.Of(kind).size(), kind == Kind::Texture ? 2u : 1u);
		return r.Of(kind).front();
	};
	EXPECT_EQ(one(Kind::Sampler).name, "s_LinearClamp");
	EXPECT_EQ(one(Kind::Sampler).binding, 1000u);
	EXPECT_EQ(one(Kind::Texture).name, "r_input_depth");
	EXPECT_EQ(one(Kind::Texture).count, 1u);
	EXPECT_EQ(r.Of(Kind::Texture)[1].name, "r_pyramid");
	EXPECT_EQ(r.Of(Kind::Texture)[1].binding, 7u);
	EXPECT_EQ(r.Of(Kind::Texture)[1].count, 3u);
	EXPECT_EQ(one(Kind::StorageTexture).name, "rw_output");
	EXPECT_EQ(one(Kind::StorageTexture).binding, 4u);
	EXPECT_EQ(one(Kind::Constants).name, "cbFI");
	EXPECT_EQ(one(Kind::Constants).binding, 5u);
	EXPECT_EQ(one(Kind::Constants).set, 0u);
	EXPECT_EQ(one(Kind::Buffer).name, "r_counters");
	EXPECT_EQ(one(Kind::StorageBuffer).name, "rw_counters");
	EXPECT_TRUE(r.read_without_format);
	EXPECT_FALSE(r.write_without_format);
}

TEST(FfxReflect, RefusesWhatIsNotAWholeModule) {
	const auto            module = MakeModule();
	FfxReflect::Reflection r;
	auto                   words = module.m_words;
	words.pop_back(); // the last instruction now runs past the end
	EXPECT_FALSE(FfxReflect::Reflect(words, r));
	words = module.m_words;
	words[0] = 0;
	EXPECT_FALSE(FfxReflect::Reflect(words, r));
	EXPECT_FALSE(FfxReflect::Reflect({}, r));
	// A bound variable of a type that is no resource.
	Module odd;
	odd.Op(22, {1, 32});
	odd.Variable(20, 1, 0, 0, "odd");
	EXPECT_FALSE(FfxReflect::Reflect(odd.m_words, r));
}

constexpr uint64_t MS = 1'000'000;

TEST(FrameGenPacer, HalfOfTheAverageInterval) {
	FrameGen::Pacer pacer;
	EXPECT_EQ(pacer.HalfNs(), 0u);
	pacer.Note(1000 * MS);
	pacer.Note(1016 * MS);
	EXPECT_EQ(pacer.HalfNs(), 0u); // one interval is not enough
	pacer.Note(1034 * MS);
	EXPECT_EQ(pacer.HalfNs(), 17 * MS / 2);
	// Only the last eight intervals count.
	uint64_t now = 1034 * MS;
	for (int i = 0; i < 8; i++) {
		pacer.Note(now += 10 * MS);
	}
	EXPECT_EQ(pacer.HalfNs(), 5 * MS);
}

TEST(FrameGenPacer, ClampsAndRestartsAfterAGap) {
	FrameGen::Pacer pacer;
	uint64_t        now = 1000 * MS;
	for (int i = 0; i < 4; i++) {
		pacer.Note(now += 1 * MS);
	}
	EXPECT_EQ(pacer.HalfNs(), 2 * MS); // 0.5 ms clamps up
	for (int i = 0; i < 8; i++) {
		pacer.Note(now += 100 * MS);
	}
	EXPECT_EQ(pacer.HalfNs(), 50 * MS);
	pacer.Note(now += 101 * MS); // a pause: measure again
	EXPECT_EQ(pacer.HalfNs(), 0u);
	pacer.Note(now += 20 * MS);
	pacer.Note(now += 20 * MS);
	EXPECT_EQ(pacer.HalfNs(), 10 * MS);
	pacer.Note(now - 1); // a clock that does not advance restarts too
	EXPECT_EQ(pacer.HalfNs(), 0u);
}

TEST(FrameGenDisplay, FifoNeedsTwiceTheGameRate) {
	using FrameGen::DisplayAllows;
	EXPECT_FALSE(DisplayAllows(true, 60'000, 60));
	EXPECT_TRUE(DisplayAllows(true, 120'000, 60));
	EXPECT_TRUE(DisplayAllows(true, 144'000, 60));
	EXPECT_TRUE(DisplayAllows(true, 119'880, 60));
	EXPECT_FALSE(DisplayAllows(true, 118'000, 60));
	EXPECT_TRUE(DisplayAllows(true, 60'000, 30));
	EXPECT_FALSE(DisplayAllows(true, 144'000, 120));
	// Unknown refresh rate: a 60 Hz display would halve the game.
	EXPECT_FALSE(DisplayAllows(true, 0, 60));
	// Without vertical sync the presenter paces itself.
	EXPECT_TRUE(DisplayAllows(false, 0, 60));
	EXPECT_TRUE(DisplayAllows(false, 60'000, 60));
}

TEST(FrameGenContinuity, ResetsWhenTheFrameBeforeWasNotDispatched) {
	FrameGen::Continuity continuity;
	auto                 step = continuity.Next(100);
	EXPECT_TRUE(step.reset); // the first one has no history
	EXPECT_EQ(step.frame_id, 1u);
	step = continuity.Next(101);
	EXPECT_FALSE(step.reset);
	EXPECT_EQ(step.frame_id, 2u);
	step = continuity.Next(103); // the title skipped a frame
	EXPECT_TRUE(step.reset);
	EXPECT_EQ(step.frame_id, 3u);
	step = continuity.Next(104);
	EXPECT_FALSE(step.reset);
	continuity.Break(); // a frame was shown without a dispatch
	step = continuity.Next(105);
	EXPECT_TRUE(step.reset);
	EXPECT_EQ(step.frame_id, 5u);
	EXPECT_FALSE(continuity.Next(106).reset);
}

TEST(FrameGenRing, FindsTheCaptureOrSaysWhyNot) {
	using FrameGen::Find;
	using FrameGen::Lookup;
	EXPECT_EQ(FrameGen::SlotOf(5), 1u);
	EXPECT_EQ(FrameGen::SlotOf(5 + FrameGen::kSlots), 1u);
	EXPECT_EQ(Find(0, 0, 0), Lookup::NoInputs);
	EXPECT_EQ(Find(7, 7, 7), Lookup::NoInputs); // the same capture as the frame before
	EXPECT_EQ(Find(8, 7, 8), Lookup::Found);
	// The render thread went once around the ring: the slot now holds a later capture.
	EXPECT_EQ(Find(8, 7, 8 + FrameGen::kSlots), Lookup::Lost);
	EXPECT_EQ(Find(8, 7, 0), Lookup::Lost); // the ring was remade
}

TEST(FrameGenRing, FrameTimeIsClamped) {
	EXPECT_FLOAT_EQ(FrameGen::FrameTimeMs(0, 5 * MS), 16.7f);
	EXPECT_FLOAT_EQ(FrameGen::FrameTimeMs(5 * MS, 5 * MS), 16.7f);
	EXPECT_FLOAT_EQ(FrameGen::FrameTimeMs(5 * MS, 25 * MS), 20.0f);
	EXPECT_FLOAT_EQ(FrameGen::FrameTimeMs(5 * MS, 5 * MS + 1000), 1.0f);
	EXPECT_FLOAT_EQ(FrameGen::FrameTimeMs(5 * MS, 5000 * MS), 250.0f);
}

TEST(FrameGenStats, OneLinePerPeriod) {
	FrameGen::Stats stats;
	EXPECT_EQ(stats.Take(), "");
	stats.Add(FrameGen::Outcome::Generated);
	stats.Add(FrameGen::Outcome::Generated);
	stats.Add(FrameGen::Outcome::Primed);
	stats.Add(FrameGen::Outcome::Lost);
	EXPECT_EQ(stats.Take(), "4 frames, 2 generated, primed 1, lost 1");
	EXPECT_EQ(stats.Take(), "");
}

#if KYTY_FSR_FRAMEGEN

constexpr auto OK      = static_cast<FfxErrorCode>(FFX_OK);
constexpr auto INVALID = static_cast<FfxErrorCode>(FFX_ERROR_INVALID_ARGUMENT);

TEST(FfxBlobs, EveryShaderReflectsIntoSetZeroWithoutClashes) {
	const auto rows = FfxBlobs::Rows();
	ASSERT_EQ(rows.size(), 36u);
	for (const auto& row: rows) {
		FfxReflect::Reflection r;
		ASSERT_TRUE(FfxReflect::Reflect(row.words, r)) << row.effect << " " << row.pass;
		std::set<uint32_t> used;
		for (const auto& kind: r.kinds) {
			for (const auto& binding: kind) {
				EXPECT_EQ(binding.set, 0u);
				EXPECT_FALSE(binding.name.empty());
				EXPECT_TRUE(used.insert(binding.binding).second) << binding.name;
			}
		}
	}
}

TEST(FfxBlobs, HandsTheBackendTheNamesItMatches) {
	FfxShaderBlob blob {};
	ASSERT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FRAMEINTERPOLATION, FFX_FRAMEINTERPOLATION_PASS_RECONSTRUCT_AND_DILATE,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0x1f, &blob),
	          OK);
	ASSERT_NE(blob.data, nullptr);
	EXPECT_EQ(blob.size % 4, 0u);
	const auto names = [](const char** list, uint32_t count) {
		return std::set<std::string>(list, list + count);
	};
	EXPECT_EQ(names(blob.boundSRVTextureNames, blob.srvTextureCount),
	          (std::set<std::string> {"r_input_depth", "r_input_motion_vectors"}));
	EXPECT_EQ(names(blob.boundUAVTextureNames, blob.uavTextureCount),
	          (std::set<std::string> {"rw_dilated_depth", "rw_dilated_motion_vectors",
	                                  "rw_reconstructed_depth_previous_frame"}));
	EXPECT_EQ(names(blob.boundConstantBufferNames, blob.cbvCount), (std::set<std::string> {"cbFI"}));
	EXPECT_EQ(blob.boundConstantBufferCounts[0], 1u);
	EXPECT_EQ(blob.boundConstantBufferSpaces[0], 0u);

	// The depth direction selects another module; the other option bits do not.
	FfxShaderBlob inverted {};
	FfxShaderBlob plain {};
	ASSERT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FRAMEINTERPOLATION, FFX_FRAMEINTERPOLATION_PASS_RECONSTRUCT_AND_DILATE,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0x3, &plain),
	          OK);
	ASSERT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FRAMEINTERPOLATION, FFX_FRAMEINTERPOLATION_PASS_RECONSTRUCT_AND_DILATE,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0x7, &inverted),
	          OK);
	EXPECT_EQ(inverted.data, blob.data);
	EXPECT_NE(plain.data, inverted.data);

	ASSERT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FRAMEINTERPOLATION, FFX_FRAMEINTERPOLATION_PASS_INTERPOLATION,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0, &blob),
	          OK);
	EXPECT_TRUE(names(blob.boundUAVTextureNames, blob.uavTextureCount).contains("rw_output"));
	EXPECT_TRUE(names(blob.boundSRVTextureNames, blob.srvTextureCount).contains("r_current_interpolation_source"));

	ASSERT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_OPTICALFLOW, FFX_OPTICALFLOW_PASS_PREPARE_LUMA,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0, &blob),
	          OK);
	EXPECT_TRUE(names(blob.boundSRVTextureNames, blob.srvTextureCount).contains("r_input_color"));

	EXPECT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FRAMEINTERPOLATION, FFX_FRAMEINTERPOLATION_PASS_COUNT,
	                                       FFX_BIND_COMPUTE_SHADER_STAGE, 0, &blob),
	          INVALID);
	EXPECT_EQ(ffxGetPermutationBlobByIndex(FFX_EFFECT_FSR1, 0, FFX_BIND_COMPUTE_SHADER_STAGE, 0, &blob),
	          INVALID);
}

#endif // KYTY_FSR_FRAMEGEN

} // namespace

int main(int argc, char** argv) {
	testing::InitGoogleTest(&argc, argv);
	return RUN_ALL_TESTS();
}
