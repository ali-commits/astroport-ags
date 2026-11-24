#include "prx/libkernel/include/ExternalRenderer.hpp"
#include <atomic>
#include <stdexcept>

namespace {

std::atomic<const ExternalRenderer*> g_renderer{nullptr};

}

extern "C" const ExternalRenderer* ExternalRendererGet_nid_no_patch() {
    return g_renderer.load(std::memory_order_acquire);
}

extern "C" void ExternalRendererSet_nid_no_patch(const ExternalRenderer* renderer) {
    if (renderer != nullptr && renderer->size < sizeof(ExternalRenderer)) throw std::invalid_argument("ExternalRenderer: the table is smaller than this libkernel expects");
    g_renderer.store(renderer, std::memory_order_release);
}
