# `nixie-counter` firmware

## Flashing

The easiest way is with cargo-embed:

    $ cargo install cargo-embed
    $ DEFMT_LOG=debug cargo embed --release

The nice advantage is that you immediately get RTT support.

Alternatively, if you prefer a simpler command, with cargo-flash:

    $ cargo install cargo-flash
    $ cargo flash --chip STM32F103C8

## Debugging

Start OpenOCD:

    $ ./openocd.sh

In another window:

    $ cargo run --release
