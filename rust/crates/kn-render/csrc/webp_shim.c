// libwebp 解码到亮度平面 (设备构建用, 见 build.rs / src/image.rs)。
// 在 C 里组装 WebPDecoderConfig, Rust 侧只需要这一个函数, 不必镜像 libwebp 的结构体布局。
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "src/webp/decode.h"

// 解码 WebP (有损或无损) 的 Y 平面 (BT.601 有限范围 16..235, 调用方再展开)。
// tw/th > 0 且图片更大时, 用 libwebp 自带的缩放器直接解码到等比放进 tw x th 的尺寸。
// 成功返回 malloc 的 w*h 字节 (行距 = w), 由 kn_webp_free 释放; 失败返回 NULL,
// *status 为 VP8StatusCode, 或 -1 = 像素数超过 max_pixels / 解码缓冲超过 max_bytes, -2 = 内存不足。
uint8_t* kn_webp_decode_luma(const uint8_t* data, size_t size, int tw, int th, uint64_t max_pixels, uint64_t max_bytes, int* ow, int* oh,
                             int* status) {
    WebPDecoderConfig config;
    *status = 0;
    if (!WebPInitDecoderConfig(&config)) {
        *status = VP8_STATUS_INVALID_PARAM;
        return NULL;
    }
    VP8StatusCode st = WebPGetFeatures(data, size, &config.input);
    if (st != VP8_STATUS_OK) {
        *status = st;
        return NULL;
    }
    int w = config.input.width, h = config.input.height;
    if (w <= 0 || h <= 0) {
        *status = VP8_STATUS_BITSTREAM_ERROR;
        return NULL;
    }
    uint64_t pixels = (uint64_t)w * (uint64_t)h;
    if (pixels > max_pixels) {
        *status = -1;
        return NULL;
    }
    // 按原始尺寸分配的内部缓冲: 无损 (或混合/动图) 整帧 ARGB 4 字节/像素, 有损带透明是 1 字节/像素的 alpha 平面;
    // 有损无透明只有行缓存。缩放不减少这部分。
    uint64_t internal = config.input.format == 1 ? (config.input.has_alpha ? pixels : 0) : pixels * 4;
    if (internal > max_bytes) {
        *status = -1;
        return NULL;
    }
    if (tw > 0 && th > 0 && (w > tw || h > th)) {
        double sx = (double)tw / w, sy = (double)th / h;
        double s = sx < sy ? sx : sy;
        int sw = (int)(w * s + 0.5), sh = (int)(h * s + 0.5);
        config.options.use_scaling = 1;
        config.options.scaled_width = sw > 0 ? sw : 1;
        config.options.scaled_height = sh > 0 ? sh : 1;
        w = config.options.scaled_width;
        h = config.options.scaled_height;
    }
    config.output.colorspace = MODE_YUV;
    st = WebPDecode(data, size, &config);
    if (st != VP8_STATUS_OK) {
        WebPFreeDecBuffer(&config.output);
        *status = st;
        return NULL;
    }
    uint8_t* out = (uint8_t*)malloc((size_t)w * (size_t)h);
    if (out == NULL) {
        WebPFreeDecBuffer(&config.output);
        *status = -2;
        return NULL;
    }
    const WebPYUVABuffer* buf = &config.output.u.YUVA;
    for (int y = 0; y < h; ++y) {
        memcpy(out + (size_t)y * w, buf->y + (size_t)y * buf->y_stride, (size_t)w);
    }
    WebPFreeDecBuffer(&config.output);
    *ow = w;
    *oh = h;
    return out;
}

void kn_webp_free(uint8_t* p) {
    free(p);
}
