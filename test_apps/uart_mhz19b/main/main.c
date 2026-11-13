/*
 * SPDX-FileCopyrightText: 2026 Espressif Systems (Shanghai) CO LTD
 *
 * SPDX-License-Identifier: Apache-2.0
 */

/* A Winsen MH-Z19B CO2 sensor read through the stock UART driver.
 *
 * The firmware side of the co-simulation UART channel: a part is the sensor,
 * and nothing here knows it is virtual. The sensor talks at 9600 baud, 8N1.
 * The read command is FF 01 86 00 00 00 00 00 79; the answer is
 * FF 86 HH LL - - - - CS, the concentration HH * 256 + LL in ppm, and CS the
 * two's complement of the sum of bytes 1 to 7 (the MH-Z19B user's manual,
 * Winsen V1.0).
 *
 * The sensor is on UART1, its line on TX 4 and RX 5 from the chip's end. On
 * the S3 it is on 17 and 18, UART1's own IO_MUX pads, which the driver routes
 * around the GPIO matrix; on the P4 on 32 and 33, bank 1 of the matrix.
 *
 * It reads the sensor three times, then prints `UART_MHZ19B: done`, then
 * reads it once a second, so a change made to the part shows in the output.
 */
#include <stdint.h>
#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/uart.h"
#include "esp_err.h"

#define PORT UART_NUM_1
#if CONFIG_IDF_TARGET_ESP32S3
#define TX_PIN 17
#define RX_PIN 18
#elif CONFIG_IDF_TARGET_ESP32P4
#define TX_PIN 32
#define RX_PIN 33
#else
#define TX_PIN 4
#define RX_PIN 5
#endif

static const uint8_t READ_CO2[9] = { 0xFF, 0x01, 0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x79 };

/* The two's complement of the sum of bytes 1 to 7. */
static uint8_t checksum(const uint8_t *frame)
{
    uint8_t sum = 0;
    for (int i = 1; i < 8; i++) {
        sum += frame[i];
    }
    return (uint8_t)(0xFF - sum + 1);
}

/* Ask for a reading: the concentration in ppm, or -1 with `why` saying what
 * went wrong. */
static int read_co2(const char **why)
{
    uint8_t answer[9];
    uart_flush_input(PORT);
    uart_write_bytes(PORT, READ_CO2, sizeof READ_CO2);
    int n = uart_read_bytes(PORT, answer, sizeof answer, pdMS_TO_TICKS(200));
    if (n != (int)sizeof answer) {
        *why = n <= 0 ? "timeout" : "short";
        return -1;
    }
    if (answer[0] != 0xFF || answer[1] != 0x86) {
        *why = "frame";
        return -1;
    }
    if (answer[8] != checksum(answer)) {
        *why = "crc";
        return -1;
    }
    return answer[2] << 8 | answer[3];
}

void app_main(void)
{
    const uart_config_t cfg = {
        .baud_rate = 9600,
        .data_bits = UART_DATA_8_BITS,
        .parity = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };
    ESP_ERROR_CHECK(uart_driver_install(PORT, 256, 0, 0, NULL, 0));
    ESP_ERROR_CHECK(uart_param_config(PORT, &cfg));
    ESP_ERROR_CHECK(uart_set_pin(PORT, TX_PIN, RX_PIN, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));

    for (int i = 0; i < 3; i++) {
        const char *why = "";
        int ppm = read_co2(&why);
        if (ppm < 0) {
            printf("MHZ19B: port=%d read=%d err=%s\n", PORT, i, why);
        } else {
            printf("MHZ19B: port=%d read=%d co2=%d crc=ok\n", PORT, i, ppm);
        }
    }
    printf("UART_MHZ19B: done\n");
    fflush(stdout);

    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(1000));
        const char *why = "";
        int ppm = read_co2(&why);
        if (ppm < 0) {
            printf("CO2: err=%s\n", why);
        } else {
            printf("CO2: %d\n", ppm);
        }
        fflush(stdout);
    }
}
