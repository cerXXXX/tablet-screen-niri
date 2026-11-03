/* ARGB/BGRx -> NV12 for the EVDI fallback path.
 *
 * The helper's fast path calls this when the compositor's framebuffer needs no
 * scaling; the caller falls back to its own portable loop whenever this returns
 * non-zero. The DMA-BUF path (the one that matters) converts on the GPU with
 * vapostproc and never gets here, so a scalar implementation is enough — but it
 * has to be *correct*, because the EVDI path is the fallback when the
 * compositor has no ext-image-copy-capture.
 *
 * Colour: BT.709, limited range (Y 16..235, CbCr 16..240), which is what the
 * encoder's VUI says and what the tablet's decoder expands back.
 *
 * `argb_order`: 1 = the bytes are B,G,R,X (DRM ARGB8888 / XRGB8888, the usual
 * case on a BGRA compositor), 0 = R,G,B,X (ABGR8888 / XBGR8888).
 */
#include <stddef.h>
#include <stdint.h>

static inline int clip8(int v, int lo, int hi) {
    return v < lo ? lo : (v > hi ? hi : v);
}

int uscreen_argb_to_nv12(const unsigned char *src, int src_stride,
                         unsigned char *y, int y_stride,
                         unsigned char *uv, int uv_stride,
                         int width, int height, int argb_order) {
    if (!src || !y || !uv || width <= 0 || height <= 0)
        return -1;

    const int ro = argb_order ? 2 : 0;
    const int bo = argb_order ? 0 : 2;

    for (int j = 0; j < height; j++) {
        const uint8_t *s = src + (size_t)j * (size_t)src_stride;
        uint8_t *yp = y + (size_t)j * (size_t)y_stride;
        for (int i = 0; i < width; i++) {
            const int r = s[(size_t)i * 4 + (size_t)ro];
            const int g = s[(size_t)i * 4 + 1];
            const int b = s[(size_t)i * 4 + (size_t)bo];
            /* BT.709 luma, 16-bit fixed point */
            const int yy = (47 * r + 157 * g + 16 * b + 128) >> 8;
            yp[i] = (uint8_t)clip8(yy + 16, 16, 235);
        }
    }

    const int cw = width / 2;
    for (int j = 0; j < height; j += 2) {
        const uint8_t *s0 = src + (size_t)j * (size_t)src_stride;
        const uint8_t *s1 = src + (size_t)((j + 1 < height) ? j + 1 : j) * (size_t)src_stride;
        uint8_t *uvp = uv + (size_t)(j / 2) * (size_t)uv_stride;
        for (int i = 0; i < cw; i++) {
            int r = 0, g = 0, b = 0;
            for (int dy = 0; dy < 2; dy++) {
                const uint8_t *s = dy ? s1 : s0;
                for (int dx = 0; dx < 2; dx++) {
                    int col = 2 * i + dx;
                    if (col >= width)
                        col = width - 1;
                    r += s[(size_t)col * 4 + (size_t)ro];
                    g += s[(size_t)col * 4 + 1];
                    b += s[(size_t)col * 4 + (size_t)bo];
                }
            }
            r = (r + 2) >> 2;
            g = (g + 2) >> 2;
            b = (b + 2) >> 2;
            /* BT.709 chroma, 16-bit fixed point */
            const int u = (-26 * r - 86 * g + 112 * b + 128) >> 8;
            const int v = (112 * r - 102 * g - 10 * b + 128) >> 8;
            uvp[2 * i] = (uint8_t)clip8(u + 128, 16, 240);
            uvp[2 * i + 1] = (uint8_t)clip8(v + 128, 16, 240);
        }
    }
    return 0;
}
