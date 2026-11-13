/*
 * SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/* A hobby servo driven by the stock LEDC driver, and on a chip with MCPWM a
 * second one driven by the stock MCPWM driver.
 *
 * The firmware side of the co-simulation PWM channel: a part is the servo,
 * and nothing here knows it is virtual. A servo takes a pulse every 20 ms;
 * the width is the angle, 1 ms at one end and 2 ms at the other (the SG90
 * datasheet). Nothing can be read back: a servo has no encoder, so what the
 * pulses meant is checked on the part's side.
 *
 * LEDC on GPIO 4: 50 Hz, 14 bits, pulses of 1000, 1500 and 2000 us, then a
 * hardware fade from 1000 to 2000 us over one second, which moves the servo
 * with no register write while it runs, then back to 1500 us. MCPWM on GPIO
 * 5, as IDF's mcpwm_servo_control example drives it (1 MHz, 20000 ticks):
 * 1000, 2000, then 1500 us. Then `PWM_SERVO: done`, both servos at the
 * middle.
 */
#include <stdint.h>
#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/ledc.h"
#include "esp_err.h"
#include "soc/soc_caps.h"
#if SOC_MCPWM_SUPPORTED
#include "driver/mcpwm_prelude.h"
#endif

#define LEDC_PIN 4
#define MCPWM_PIN 5

#define SERVO_HZ 50
#define LEDC_BITS LEDC_TIMER_14_BIT
#define LEDC_STEPS (1 << 14)
#define PERIOD_US (1000000 / SERVO_HZ)

/* How long each position is held: long enough for the part to be told. */
#define HOLD_MS 200

/* Duty steps for a pulse of `us`, rounded to the nearest step (1.22 us). */
static uint32_t ledc_duty(int us)
{
    return (uint32_t)(((int64_t)us * LEDC_STEPS + PERIOD_US / 2) / PERIOD_US);
}

static void ledc_pulse(int us)
{
    ESP_ERROR_CHECK(ledc_set_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0, ledc_duty(us)));
    ESP_ERROR_CHECK(ledc_update_duty(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0));
    printf("PWM_SERVO: ledc pulse_us=%d\n", us);
    vTaskDelay(pdMS_TO_TICKS(HOLD_MS));
}

static void ledc_servo(void)
{
    ledc_timer_config_t timer = {
        .speed_mode = LEDC_LOW_SPEED_MODE,
        .timer_num = LEDC_TIMER_0,
        .duty_resolution = LEDC_BITS,
        .freq_hz = SERVO_HZ,
        .clk_cfg = LEDC_AUTO_CLK,
    };
    ESP_ERROR_CHECK(ledc_timer_config(&timer));
    ledc_channel_config_t channel = {
        .gpio_num = LEDC_PIN,
        .speed_mode = LEDC_LOW_SPEED_MODE,
        .channel = LEDC_CHANNEL_0,
        .timer_sel = LEDC_TIMER_0,
        .duty = 0,
        .hpoint = 0,
    };
    ESP_ERROR_CHECK(ledc_channel_config(&channel));

    ledc_pulse(1000);
    ledc_pulse(1500);
    ledc_pulse(2000);

    /* The hardware steps the duty itself: no register write until it ends. */
    ESP_ERROR_CHECK(ledc_fade_func_install(0));
    ledc_pulse(1000);
    ESP_ERROR_CHECK(ledc_set_fade_with_time(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0, ledc_duty(2000), 1000));
    ESP_ERROR_CHECK(ledc_fade_start(LEDC_LOW_SPEED_MODE, LEDC_CHANNEL_0, LEDC_FADE_WAIT_DONE));
    printf("PWM_SERVO: ledc fade 1000->2000 done\n");
    vTaskDelay(pdMS_TO_TICKS(HOLD_MS));
    ledc_pulse(1500);
}

#if SOC_MCPWM_SUPPORTED
static void mcpwm_servo(void)
{
    mcpwm_timer_handle_t timer = NULL;
    mcpwm_timer_config_t timer_config = {
        .group_id = 0,
        .clk_src = MCPWM_TIMER_CLK_SRC_DEFAULT,
        .resolution_hz = 1000000,
        .period_ticks = PERIOD_US,
        .count_mode = MCPWM_TIMER_COUNT_MODE_UP,
    };
    ESP_ERROR_CHECK(mcpwm_new_timer(&timer_config, &timer));
    mcpwm_oper_handle_t oper = NULL;
    mcpwm_operator_config_t operator_config = { .group_id = 0 };
    ESP_ERROR_CHECK(mcpwm_new_operator(&operator_config, &oper));
    ESP_ERROR_CHECK(mcpwm_operator_connect_timer(oper, timer));
    mcpwm_cmpr_handle_t comparator = NULL;
    mcpwm_comparator_config_t comparator_config = { .flags.update_cmp_on_tez = true };
    ESP_ERROR_CHECK(mcpwm_new_comparator(oper, &comparator_config, &comparator));
    mcpwm_gen_handle_t generator = NULL;
    mcpwm_generator_config_t generator_config = { .gen_gpio_num = MCPWM_PIN };
    ESP_ERROR_CHECK(mcpwm_new_generator(oper, &generator_config, &generator));

    ESP_ERROR_CHECK(mcpwm_comparator_set_compare_value(comparator, 1000));
    /* High at the timer's zero, low at the compare value: the pulse. */
    ESP_ERROR_CHECK(mcpwm_generator_set_action_on_timer_event(generator,
        MCPWM_GEN_TIMER_EVENT_ACTION(MCPWM_TIMER_DIRECTION_UP, MCPWM_TIMER_EVENT_EMPTY, MCPWM_GEN_ACTION_HIGH)));
    ESP_ERROR_CHECK(mcpwm_generator_set_action_on_compare_event(generator,
        MCPWM_GEN_COMPARE_EVENT_ACTION(MCPWM_TIMER_DIRECTION_UP, comparator, MCPWM_GEN_ACTION_LOW)));
    ESP_ERROR_CHECK(mcpwm_timer_enable(timer));
    ESP_ERROR_CHECK(mcpwm_timer_start_stop(timer, MCPWM_TIMER_START_NO_STOP));

    const int pulses[] = { 1000, 2000, 1500 };
    for (int i = 0; i < 3; i++) {
        ESP_ERROR_CHECK(mcpwm_comparator_set_compare_value(comparator, pulses[i]));
        printf("PWM_SERVO: mcpwm pulse_us=%d\n", pulses[i]);
        vTaskDelay(pdMS_TO_TICKS(HOLD_MS));
    }
}
#endif

void app_main(void)
{
#if SOC_MCPWM_SUPPORTED
    printf("PWM_SERVO: ledc on GPIO %d, mcpwm on GPIO %d\n", LEDC_PIN, MCPWM_PIN);
#else
    printf("PWM_SERVO: ledc on GPIO %d, no mcpwm\n", LEDC_PIN);
#endif
    ledc_servo();
#if SOC_MCPWM_SUPPORTED
    mcpwm_servo();
#endif
    printf("PWM_SERVO: done\n");
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(1000));
    }
}
