# Nixie Counter

## Description / Overview

PCB for a physical "people now present" counter that sends it data directly to
the [SpaceAPI](https://spaceapi.io/).

![Photo without enclosure](nixie_counter.jpg)

![Photo with enclosure](nixie_counter_enclosure.jpg)

**Microcontroller**

The microcontroller is an ESP32C3 based [M5Stack
STAMP-C3U](https://docs.m5stack.com/en/core/stamp_c3u) board. We chose this
because it's quite cheap, has everything built-in that we need and it can run
firmware written in [Rust](https://www.rust-lang.org/).

**Input Methods**

To enter the number of people present, a 2-position momentary toggle switch is
used that can be pushed upwards or downwards.

**Display**

The number of people present is then displayed on two IN-12B nixie tubes. Those
tubes need 150-160 V for turning on, for this we use a [NCH6100HV
module](https://www.nixie.ai/nch6100hv/). The tubes are controlled through two
K155ID1 BCD-to-Decimal drivers. Those need 5V logic levels, so we use a level
shifter between the drivers and the microcontroller.

**Power Supply**

Input power is supplied at 12V. The nixie power supply converts that to ~150V.

An LDOs convert the 12V to 5V, for both the nixie drivers and the ESP32 module.

**Status LEDs**

There's a yellow LED to indicate that the controller has power and runs the
correct firmware, and a green LED to indicate whether the module is connected
to the WiFi or not.

## PCB

![Top](output/v1.1/screenshot-top.png)

![Bottom](output/v1.1/screenshot-bot.png)

## Editing

This is a [LibrePCB](https://librepcb.org) project!

## License

TAPR Open Hardware License, see [LICENSE.txt](LICENSE.txt).
