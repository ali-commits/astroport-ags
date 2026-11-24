# libAstroportHost technical debt

### Modelled behaviour

- [event_add, event_delete](../../../core/libs/prx/libAstroportHost/Export.cpp) (libAstroportHost) - a zero or unknown event queue and an unknown event kind return `0x80020009` (`SCE_KERNEL_ERROR_EBADF`). `sceAgcDriverAddEqEvent` and `sceVideoOutAddFlipEvent` in this tree throw for the same input; the host table is called from outside AnyPS5 and no exception may cross it. The console's result is not measured.
- [flip events](../../../core/libs/prx/libAstroportHost/Export.cpp) (libAstroportHost) - the event data is the payload of the external renderer, unchanged; no counter or time stamp is added as `libSceVideoOut` does. Every flip registration receives every flip, whatever video-out handle it was registered with. Vblank events are not modelled.
- [pad_fetch_output](../../../core/libs/prx/libAstroportHost/Export.cpp) (libAstroportHost) - trigger effects are not reported: `libScePad` keeps its own DualSense encoding and not the 56-byte command of `scePadSetTriggerEffect`, so `triggerSet` stays 0.
- [AgcDriverHostHdr, AgcDriverNoteReportedToneMapPeak](../../../core/libs/prx/libAstroportHost/Export.cpp) (libAstroportHost) - the display is always reported as SDR; the external renderer is not asked.
