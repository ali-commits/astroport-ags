# Notices

## 1. AstroPort

The port's own code (`patcher/`, `tools/`, `scripts/`, `patches/`) and both engines are licensed under the GNU
General Public License, version 2 only (`GPL-2.0-only`). See [LICENSE](LICENSE).

## 2. Engines

| Engine | Upstream | Licence files | Base and snapshot | Details |
|---|---|---|---|---|
| `engines/kyty` | [Jetsku/KytyPS5](https://github.com/Jetsku/KytyPS5), based on Kyty by InoriRus | `engines/kyty/LICENSE` (GPL-2.0-only), `engines/kyty/LICENSES/Kyty-MIT.txt` (Kyty's original MIT notice) | Jetsku int22 `dd9f9901`; snapshot `457b1fa8` | [UPSTREAM.md](engines/kyty/UPSTREAM.md) |
| `engines/anyps5` | [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5) | `engines/anyps5/LICENSE` (GPL-2.0-only) | base `319621ba2e`; snapshot `2429864775` | [UPSTREAM.md](engines/anyps5/UPSTREAM.md) |

## 3. AstroHybrid

AstroHybrid is by faycal (https://github.com/imnotsharingmyname/pcportastrobot). It wrote the bridge that runs
Kyty's renderer inside AnyPS5, and most of the engine changes in both. Its changes are modifications of the GPL-2.0-only engines above and are distributed here under the same licence.

## 4. Third-party libraries

Each library keeps its own licence, at the path given. The folder `engines/kyty/3rdparty/patches` holds build patches, not a library.
`engines/anyps5/3rdparty/Vulkan-Headers` and `engines/anyps5/3rdparty/ffmpeg-core` are symlinks to Kyty's copies below.

### engines/kyty/3rdparty

- cpuinfo (vendored copy, no upstream pin recorded): `engines/kyty/3rdparty/cpuinfo/LICENSE`
- ffmpeg-core (https://github.com/KytyPS5/ext-ffmpeg-core): `engines/kyty/3rdparty/ffmpeg-core/copyright`
- fmt (https://github.com/fmtlib/fmt): `engines/kyty/3rdparty/fmt/LICENSE`
- imgui (https://github.com/ocornut/imgui): `engines/kyty/3rdparty/imgui/LICENSE.txt`
- LibAtrac9 (https://github.com/shadps4-emu/ext-LibAtrac9): `engines/kyty/3rdparty/LibAtrac9/LICENSE`
- magic_enum (https://github.com/Neargye/magic_enum): `engines/kyty/3rdparty/magic_enum/LICENSE`
- nlohmann_json (https://github.com/nlohmann/json): `engines/kyty/3rdparty/nlohmann_json/LICENSE.MIT`
- renderdoc_app.h (https://github.com/baldurk/renderdoc): the MIT licence is in the header of `engines/kyty/3rdparty/renderdoc/renderdoc_app.h`.
- SDL3 (https://github.com/libsdl-org/SDL): `engines/kyty/3rdparty/SDL3/LICENSE.txt`
- spdlog (https://github.com/gabime/spdlog): `engines/kyty/3rdparty/spdlog/LICENSE`
- SPIRV-Headers (https://github.com/KhronosGroup/SPIRV-Headers): `engines/kyty/3rdparty/SPIRV-Headers/LICENSE`
- SPIRV-Tools (https://github.com/KhronosGroup/SPIRV-Tools): `engines/kyty/3rdparty/SPIRV-Tools/LICENSE`
- stb (https://github.com/nothings/stb, vendored copy): `engines/kyty/3rdparty/stb/LICENSE`
- tracy (https://github.com/wolfpld/tracy): `engines/kyty/3rdparty/tracy/LICENSE`
- Vulkan-Headers (https://github.com/KhronosGroup/Vulkan-Headers): `engines/kyty/3rdparty/Vulkan-Headers/LICENSE.md`
- VulkanMemoryAllocator (https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator): `engines/kyty/3rdparty/VulkanMemoryAllocator/LICENSE.txt`
- winpthread: licence only, `engines/kyty/3rdparty/winpthread/COPYING`. Not built on Linux (clang-cl only).
- xxHash (https://github.com/Cyan4973/xxHash): `engines/kyty/3rdparty/xxHash/LICENSE`

### engines/anyps5/3rdparty

- freetype (https://github.com/freetype/freetype): `engines/anyps5/3rdparty/freetype/LICENSE.TXT`
- glslang (https://github.com/KhronosGroup/glslang): `engines/anyps5/3rdparty/glslang/LICENSE.txt`
- jxrlib (https://github.com/4creators/jxrlib): `engines/anyps5/3rdparty/jxrlib/LICENSE`
- LibAtrac9 (https://github.com/shadps4-emu/ext-LibAtrac9): `engines/anyps5/3rdparty/LibAtrac9/LICENSE`
- libjpeg-turbo (https://github.com/libjpeg-turbo/libjpeg-turbo): `engines/anyps5/3rdparty/libjpeg-turbo/LICENSE.md`
- libusb-cmake (https://github.com/libusb/libusb-cmake): `engines/anyps5/3rdparty/libusb-cmake/LICENSE`
- mbedtls (https://github.com/Mbed-TLS/mbedtls): `engines/anyps5/3rdparty/mbedtls/LICENSE`
- mimalloc (https://github.com/microsoft/mimalloc): `engines/anyps5/3rdparty/mimalloc/LICENSE`
- nghttp2 (https://github.com/nghttp2/nghttp2): `engines/anyps5/3rdparty/nghttp2/COPYING`
- ogg (https://github.com/xiph/ogg): `engines/anyps5/3rdparty/ogg/COPYING`
- opus (https://github.com/xiph/opus): `engines/anyps5/3rdparty/opus/COPYING`
- SDL2 (https://github.com/libsdl-org/SDL): `engines/anyps5/3rdparty/SDL2/LICENSE.txt`
- SPIRV-Headers (https://github.com/KhronosGroup/SPIRV-Headers): `engines/anyps5/3rdparty/SPIRV-Headers/LICENSE`
- SPIRV-Tools (https://github.com/KhronosGroup/SPIRV-Tools): `engines/anyps5/3rdparty/SPIRV-Tools/LICENSE`
- stb (https://github.com/nothings/stb, vendored copy): `engines/anyps5/3rdparty/stb/LICENSE`
- tracy (https://github.com/wolfpld/tracy): `engines/anyps5/3rdparty/tracy/LICENSE`
- vgmstream (https://github.com/vgmstream/vgmstream): `engines/anyps5/3rdparty/vgmstream/COPYING`
- vorbis (https://github.com/xiph/vorbis): `engines/anyps5/3rdparty/vorbis/COPYING`
- VulkanMemoryAllocator (https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator): `engines/anyps5/3rdparty/VulkanMemoryAllocator/LICENSE.txt`
- ZArchive (https://github.com/Exzap/ZArchive): `engines/anyps5/3rdparty/ZArchive/LICENSE`
- zstd (https://github.com/facebook/zstd): `engines/anyps5/3rdparty/zstd/COPYING`

Fetched at configure time (Kyty's `3rdparty/CMakeLists.txt`; not in this repository, each keeps its own licence):
opus, OpenXR-SDK, xbyak, zydis, zstd, ZArchive (with `engines/kyty/3rdparty/patches/zarchive-reader.patch`) and Noto Sans.

## 5. NVIDIA DLSS

Release packages also contain a DLSS-enabled renderer built with NVIDIA's DLSS SDK, and NVIDIA's DLSS runtime
where the SDK licence allows it to be redistributed. Those parts are under NVIDIA's DLSS SDK licence, which is
not compatible with the GPL. Distributing them together with GPL-2.0 code is a known licence risk that this
project accepts. The SDK is not in this repository: the release build fetches it from
https://github.com/NVIDIA/DLSS, and a source build without it has no DLSS. DLSS is offered on NVIDIA GPUs only.

## 6. Also used

lsfg-vk provides frame generation on Linux. It is not bundled.

## 7. Trademarks

"Astro Bot" and "PlayStation" are trademarks of their owners. This is a fan project. It is not affiliated with, endorsed by, or supported by Sony Interactive Entertainment or Team Asobi.
