/*
 * SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/* The stock esp_driver_cam DVP controller against a co-simulated sensor:
 * FRAMES raw RGB565 frames, then FRAMES JPEG frames, each reported with its
 * size and CRC32 for the host to compare with what it sent. The sensor
 * switches to JPEG when the receiver starts the second time. */

#include <stdio.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "esp_cam_ctlr.h"
#include "esp_cam_ctlr_dvp.h"
#include "esp_heap_caps.h"
#include "esp_rom_crc.h"

#define H_RES   64
#define V_RES   48
#define FRAMES  3

/* The same pads on every target that has the block. */
static const esp_cam_ctlr_dvp_pin_config_t s_pins = {
    .data_width = CAM_CTLR_DATA_WIDTH_8,
    .data_io = {4, 5, 6, 7, 8, 9, 10, 11},
    .pclk_io = 12,
    .vsync_io = 13,
    .de_io = 14,
    .xclk_io = 15,
};

typedef struct {
    size_t size;
    uint32_t crc;
} frame_t;

static QueueHandle_t s_done;
static void *s_buf;
static size_t s_buf_len;

static bool on_get_new_trans(esp_cam_ctlr_handle_t handle, esp_cam_ctlr_trans_t *trans, void *user)
{
    trans->buffer = s_buf;
    trans->buflen = s_buf_len;
    return false;
}

/* The driver hands the one buffer straight back to the DMA, so the frame is
 * summed here, before the next one lands in it. */
static bool on_trans_finished(esp_cam_ctlr_handle_t handle, esp_cam_ctlr_trans_t *trans, void *user)
{
    BaseType_t woken = pdFALSE;
    frame_t f = {
        .size = trans->received_size,
        .crc = esp_rom_crc32_le(0, trans->buffer, trans->received_size),
    };
    xQueueSendFromISR(s_done, &f, &woken);
    return woken == pdTRUE;
}

static bool capture(bool jpeg)
{
    esp_cam_ctlr_dvp_config_t cfg = {
        .ctlr_id = 0,
        .clk_src = CAM_CLK_SRC_DEFAULT,
        .h_res = H_RES,
        .v_res = V_RES,
        .input_data_color_type = CAM_CTLR_COLOR_RGB565,
        .output_data_color_type = CAM_CTLR_COLOR_RGB565,
        .dma_burst_size = 64,
        .pin = &s_pins,
        .bk_buffer_dis = 1,
        .pic_format_jpeg = jpeg,
        .xclk_freq = 20000000,
    };
    esp_cam_ctlr_handle_t cam = NULL;
    ESP_ERROR_CHECK(esp_cam_new_dvp_ctlr(&cfg, &cam));
    ESP_ERROR_CHECK(esp_cam_ctlr_get_frame_buffer_len(cam, &s_buf_len));
    /* External RAM, as IDF's own example: the driver syncs the buffer's cache,
     * and on S3 and S31 internal RAM has none to sync. */
    s_buf = esp_cam_ctlr_alloc_buffer(cam, s_buf_len, MALLOC_CAP_DMA | MALLOC_CAP_SPIRAM);
    if (!s_buf) {
        printf("no frame buffer of %u bytes\n", (unsigned)s_buf_len);
        return false;
    }
    esp_cam_ctlr_evt_cbs_t cbs = {
        .on_get_new_trans = on_get_new_trans,
        .on_trans_finished = on_trans_finished,
    };
    ESP_ERROR_CHECK(esp_cam_ctlr_register_event_callbacks(cam, &cbs, NULL));
    ESP_ERROR_CHECK(esp_cam_ctlr_enable(cam));
    ESP_ERROR_CHECK(esp_cam_ctlr_start(cam));

    bool ok = true;
    for (int i = 0; i < FRAMES; i++) {
        frame_t f;
        if (xQueueReceive(s_done, &f, pdMS_TO_TICKS(5000)) != pdTRUE) {
            printf("%s frame %d: timeout\n", jpeg ? "JPEG" : "RAW", i);
            ok = false;
            break;
        }
        printf("%s frame %d: size=%u crc=%08x\n", jpeg ? "JPEG" : "RAW", i, (unsigned)f.size, (unsigned)f.crc);
        ok = ok && f.size > 0;
    }
    ESP_ERROR_CHECK(esp_cam_ctlr_stop(cam));
    ESP_ERROR_CHECK(esp_cam_ctlr_disable(cam));
    ESP_ERROR_CHECK(esp_cam_ctlr_del(cam));
    heap_caps_free(s_buf);
    return ok;
}

void app_main(void)
{
    s_done = xQueueCreate(8, sizeof(frame_t));
    bool ok = capture(false);
    xQueueReset(s_done);
    ok = capture(true) && ok;
    printf("CAMERA_DVP %s\n", ok ? "PASSED" : "FAILED");
}
