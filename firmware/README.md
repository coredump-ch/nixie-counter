# Nixie Counter Firmware

Prerequisites:

- Rust
- Cargo
- espflash
- ldproxy

Flashing:

    export WIFI_SSID=...
    export WIFI_PASS=...
    export SPACEAPI_SENSOR_ENDPOINT=http://example.com/sensors/people_now_present/
    cargo run --release
