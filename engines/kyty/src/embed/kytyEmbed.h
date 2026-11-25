#ifndef KYTY_EMBED_KYTYEMBED_H_
#define KYTY_EMBED_KYTYEMBED_H_

/* Kyty's renderer embedded in a host process: the one exported symbol of libkyty_renderer.so.
 * Plain C data only. Pointers passed to a call are valid during the call (config strings: for the
 * process). Fields are only ever appended; a consumer checks abi (exact) and size (>=). */

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define KYTY_EMBED_ABI 1

typedef struct KytyEmbedConfig {
	uint32_t size;
	uint32_t width, height;           /* 0 = Kyty default */
	uint32_t vblank_hz;               /* 0 = 60 */
	uint32_t vibration_intensity;     /* 0..100 */
	uint32_t rumble_scale;            /* percent 0..200 */
	uint32_t rumble_switch_floor;     /* 0..255 */
	uint32_t low_latency_interval_us; /* initial value; 0 = none */
	uint32_t dlss_preset;             /* 0 = model default */
	uint64_t upscale_pass_hash;       /* pixel shader hash of the pass DLSS replaces; 0 = none */
	uint8_t  fullscreen;
	uint8_t  present_mode;            /* 0 immediate 1 fifo 2 mailbox */
	uint8_t  shader_optimization;     /* 0 none 1 size 2 performance */
	uint8_t  wayland_native, low_latency, dlss, dlss_reverse_depth;
	uint8_t  pad_prefer;              /* 0 off 1 auto 2 ps 3 xbox 4 switch 5 generic */
	uint8_t  ds_rumble, fsr_fg;
	const char* app0_dir;     /* UTF-8; sce_sys/param.json, icon0.png, eboot.bin are read from it */
	const char* window_title; /* NULL = Kyty's diagnostic title */
	const char* dlss_dir;     /* folder of the DLSS runtime (read-only is fine) */
	const char* ngx_data_dir; /* writable folder for NGX */
	const char* fsr_dir;
	/* The upscale pass, as indices: the constant words of jitter x, jitter y, output width, output
	 * height, input width, input height, frame counter and "motion vectors in use"; the motion
	 * image and the colour image among the pixel shader's images; the constant buffer among its
	 * buffers; then 1 when this layout is given (0 = DLSS cannot engage). */
	uint8_t upscale_pass_layout[16];
} KytyEmbedConfig;

typedef struct KytyEmbedHooks {
	uint32_t size;
	void (*reserved)(void); /* guest address space is reserved; run thread */
	void (*ready)(void);    /* all subsystems are up; run thread */
	/* 0 = present now; >0 = do not present before this tick */
	int64_t (*present_gate)(int64_t now, int64_t ticks_per_s, int flip_ready);
	void (*presented)(int64_t now, int64_t ticks_per_s);
	const char* (*live_setting)(const char* apply_json); /* reply JSON, valid until the next call */
} KytyEmbedHooks;

typedef struct KytyEmbedFns { /* addresses of upstream KYTY_SYSV_ABI functions */
	void *pad_init, *pad_open, *pad_get_handle, *pad_read_state, *pad_set_vibration,
	    *pad_set_light_bar, *pad_reset_light_bar, *pad_set_trigger_effect;
	void *equeue_create, *equeue_wait, *equeue_add_user_event, *equeue_trigger_user_event;
	void *agc_add_eq_event, *video_out_add_flip_event;
} KytyEmbedFns;

typedef struct KytyEmbedApi {
	uint32_t            abi, size;
	uint32_t            pad_data_size; /* sizeof(Libs::Controller::PadData) = 120 */
	const KytyEmbedFns* fns;
	/* Blocks until the window loop ends; the calling thread becomes Kyty's main thread. */
	int   (*run)(const KytyEmbedConfig*, const KytyEmbedHooks*);
	void* (*find)(const char* nid);
	int   (*fault)(uint64_t address, int access /*0 read 1 write 2 execute*/, uint64_t instruction);
	void  (*quit)(void);
	void  (*flush)(void);
	int   (*user_id)(void);
	/* microseconds queued; 0 = refused */
	uint64_t (*pad_audio)(uint32_t rate, int speaker, const void* samples, uint32_t frames,
	                      uint32_t channels, int is_float, float gain);
	void (*set_low_latency_interval)(uint32_t us);
	void (*set_window_size)(uint32_t width, uint32_t height);
} KytyEmbedApi;

const KytyEmbedApi* kyty_embed_api(void);

#ifdef __cplusplus
}
#endif

#endif /* KYTY_EMBED_KYTYEMBED_H_ */
