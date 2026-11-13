/*
 * SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/* A KY-040 rotary encoder counted by PCNT through the stock esp_driver_pcnt.
 *
 * The unit is set up as ESP-IDF's examples/peripherals/pcnt/rotary_encoder
 * sets it: a 1 us glitch filter, channel A counting CLK's edges gated by DT's
 * level, channel B the mirror. That is four counts a detent, clockwise up.
 * The switch is read through a GPIO interrupt on any edge.
 *
 * What it prints, one line each:
 *   ENCODER: ready clk=5 dt=6 sw=7      once the unit is counting
 *   ENCODER: count=N                    each change seen by the 10 ms poll
 *   ENCODER: watch=N                    each watch-point event, +-8 (two detents)
 *   ENCODER: switch=pressed / released  each switch edge
 */
#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "driver/pulse_cnt.h"
#include "driver/gpio.h"

#define CLK_GPIO 5
#define DT_GPIO  6
#define SW_GPIO  7

#define HIGH_LIMIT  100
#define LOW_LIMIT  -100
#define WATCH       8

/* Queues, not variables: a quick turn can cross a watch point and come back
 * between two polls, and each event gets its line. */
static QueueHandle_t watch_q;
static QueueHandle_t switch_q;

static bool on_reach(pcnt_unit_handle_t unit, const pcnt_watch_event_data_t *ev, void *ctx)
{
    BaseType_t woken = pdFALSE;
    int at = ev->watch_point_value;
    xQueueSendFromISR(watch_q, &at, &woken);
    return woken == pdTRUE;
}

static void IRAM_ATTR on_switch(void *arg)
{
    int level = gpio_get_level(SW_GPIO);
    xQueueSendFromISR(switch_q, &level, NULL);
}

void app_main(void)
{
    watch_q = xQueueCreate(16, sizeof(int));
    switch_q = xQueueCreate(8, sizeof(int));

    pcnt_unit_config_t unit_cfg = { .high_limit = HIGH_LIMIT, .low_limit = LOW_LIMIT };
    pcnt_unit_handle_t unit = NULL;
    ESP_ERROR_CHECK(pcnt_new_unit(&unit_cfg, &unit));
    pcnt_glitch_filter_config_t filter = { .max_glitch_ns = 1000 };
    ESP_ERROR_CHECK(pcnt_unit_set_glitch_filter(unit, &filter));

    /* The module has pull-ups on CLK and DT; the chip's go on too, as the
     * example does without them. */
    ESP_ERROR_CHECK(gpio_pullup_en(CLK_GPIO));
    ESP_ERROR_CHECK(gpio_pullup_en(DT_GPIO));

    pcnt_chan_config_t a_cfg = { .edge_gpio_num = CLK_GPIO, .level_gpio_num = DT_GPIO };
    pcnt_channel_handle_t chan_a = NULL;
    ESP_ERROR_CHECK(pcnt_new_channel(unit, &a_cfg, &chan_a));
    pcnt_chan_config_t b_cfg = { .edge_gpio_num = DT_GPIO, .level_gpio_num = CLK_GPIO };
    pcnt_channel_handle_t chan_b = NULL;
    ESP_ERROR_CHECK(pcnt_new_channel(unit, &b_cfg, &chan_b));
    /* (rising, falling) and (high, low), as the example has them. */
    ESP_ERROR_CHECK(pcnt_channel_set_edge_action(chan_a, PCNT_CHANNEL_EDGE_ACTION_DECREASE, PCNT_CHANNEL_EDGE_ACTION_INCREASE));
    ESP_ERROR_CHECK(pcnt_channel_set_level_action(chan_a, PCNT_CHANNEL_LEVEL_ACTION_KEEP, PCNT_CHANNEL_LEVEL_ACTION_INVERSE));
    ESP_ERROR_CHECK(pcnt_channel_set_edge_action(chan_b, PCNT_CHANNEL_EDGE_ACTION_INCREASE, PCNT_CHANNEL_EDGE_ACTION_DECREASE));
    ESP_ERROR_CHECK(pcnt_channel_set_level_action(chan_b, PCNT_CHANNEL_LEVEL_ACTION_KEEP, PCNT_CHANNEL_LEVEL_ACTION_INVERSE));

    ESP_ERROR_CHECK(pcnt_unit_add_watch_point(unit, WATCH));
    ESP_ERROR_CHECK(pcnt_unit_add_watch_point(unit, -WATCH));
    pcnt_event_callbacks_t cbs = { .on_reach = on_reach };
    ESP_ERROR_CHECK(pcnt_unit_register_event_callbacks(unit, &cbs, NULL));
    ESP_ERROR_CHECK(pcnt_unit_enable(unit));
    ESP_ERROR_CHECK(pcnt_unit_clear_count(unit));
    ESP_ERROR_CHECK(pcnt_unit_start(unit));

    gpio_config_t sw_cfg = {
        .pin_bit_mask = 1ULL << SW_GPIO,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .intr_type = GPIO_INTR_ANYEDGE,
    };
    ESP_ERROR_CHECK(gpio_config(&sw_cfg));
    ESP_ERROR_CHECK(gpio_install_isr_service(0));
    ESP_ERROR_CHECK(gpio_isr_handler_add(SW_GPIO, on_switch, NULL));

    printf("ENCODER: ready clk=%d dt=%d sw=%d\n", CLK_GPIO, DT_GPIO, SW_GPIO);
    fflush(stdout);

    int last = 0;
    for (;;) {
        int v;
        while (xQueueReceive(watch_q, &v, 0) == pdTRUE) {
            printf("ENCODER: watch=%d\n", v);
        }
        while (xQueueReceive(switch_q, &v, 0) == pdTRUE) {
            printf("ENCODER: switch=%s\n", v ? "released" : "pressed");
        }
        int count = 0;
        ESP_ERROR_CHECK(pcnt_unit_get_count(unit, &count));
        if (count != last) {
            printf("ENCODER: count=%d\n", count);
            last = count;
        }
        fflush(stdout);
        vTaskDelay(pdMS_TO_TICKS(10));
    }
}
