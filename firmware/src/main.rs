use std::time::Duration;

use anyhow::Context;
use embedded_svc::{
    http::client::Client as HttpClient,
    wifi::{AuthMethod, ClientConfiguration, Configuration},
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        gpio::{IOPin, OutputPin, PinDriver},
        prelude::Peripherals,
        task::block_on,
    },
    http::client::EspHttpConnection,
    io::Write,
    nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
    wifi::{AsyncWifi, EspWifi},
};

mod nixie;
mod toggle_switch;

use nixie::{NixieTube, NixieTubePair};
use toggle_switch::{Direction, ToggleSwitch};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const WIFI_SSID: &str = env!("WIFI_SSID");
const WIFI_PASS: &str = env!("WIFI_PASS");
const SPACEAPI_SENSOR_ENDPOINT: &str = env!("SPACEAPI_SENSOR_ENDPOINT");

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    log::info!("Starting nixie firmware v{VERSION}...");

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let timer_service = EspTaskTimerService::new()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // Create async timer
    let mut timer = timer_service
        .timer_async()
        .context("Failed to initialize timer")?;

    // Set up toggle switch
    let mut toggle_switch = ToggleSwitch::new(
        peripherals.pins.gpio1.downgrade(),
        peripherals.pins.gpio0.downgrade(),
    )?;

    // Set up LEDs
    let mut led_pwr = PinDriver::output(peripherals.pins.gpio20)?;
    let mut led_wifi = PinDriver::output(peripherals.pins.gpio21)?;
    led_pwr.set_high().context("Could not enable power LED")?;

    // Initialize tubes
    let mut tubes = NixieTubePair::new(
        NixieTube {
            pin_a: PinDriver::output(peripherals.pins.gpio6.downgrade_output())?,
            pin_b: PinDriver::output(peripherals.pins.gpio4.downgrade_output())?,
            pin_c: PinDriver::output(peripherals.pins.gpio3.downgrade_output())?,
            pin_d: PinDriver::output(peripherals.pins.gpio5.downgrade_output())?,
        },
        NixieTube {
            pin_a: PinDriver::output(peripherals.pins.gpio9.downgrade_output())?,
            pin_b: PinDriver::output(peripherals.pins.gpio8.downgrade_output())?,
            pin_c: PinDriver::output(peripherals.pins.gpio7.downgrade_output())?,
            pin_d: PinDriver::output(peripherals.pins.gpio10.downgrade_output())?,
        },
    );
    block_on(async {
        tubes.selftest(&mut timer, Duration::from_millis(100)).await;
    });

    // Connect WiFi
    let mut wifi = AsyncWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))
            .context("Failed to instantiate EspWifi")?,
        sys_loop,
        timer_service,
    )?;
    block_on(connect_wifi(&mut wifi))?;
    led_wifi.set_high().context("Could not enable WiFi LED")?;

    // Create HTTP client (without TLS support for now)
    let mut client = HttpClient::wrap(EspHttpConnection::new(&Default::default())?);

    // Main loop
    let mut count = 0usize;
    block_on(async {
        loop {
            // Wait for toggle switch press
            let direction = toggle_switch.wait_for_press().await;
            log::info!("Pressed {:?}", direction);

            // Debouncing
            if let Err(e) = timer.after(Duration::from_millis(100)).await {
                log::error!("Failed to wait for debouncing delay: {e}")
            }

            // Update SpaceAPI
            match direction {
                Direction::Up => count = count.saturating_add(1),
                Direction::Down => count = count.saturating_sub(1),
            }
            update_people_now_present(&mut client, count)?;

            // Update nixie tubes
            tubes.show(
                count
                    .min(99)
                    .try_into()
                    .expect("Failed to convert count to u8"),
            );

            // Wait for toggle switch release
            toggle_switch
                .wait_for_release()
                .await
                .context("Failed to wait for toggle release")?;
        }
    })
}

/// Connect to WiFi
///
/// Credentials must be passed in through env variables (WIFI_SSID and WIFI_PASS).
async fn connect_wifi(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
        ssid: WIFI_SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: WIFI_PASS.try_into().unwrap(),
        channel: None,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;

    wifi.start().await?;
    log::info!("Wifi started");

    wifi.connect().await?;
    log::info!("Wifi connected");

    wifi.wait_netif_up().await?;
    log::info!("Wifi netif up");

    Ok(())
}

/// Update the "people now present" sensor through HTTP.
fn update_people_now_present(
    client: &mut HttpClient<EspHttpConnection>,
    people_count: usize,
) -> anyhow::Result<()> {
    // Prepare payload
    let payload_string = format!("value={people_count}");
    let payload = payload_string.as_bytes();

    // Prepare headers and URL
    let content_length_header = format!("{}", payload.len());
    let headers = [
        ("content-type", "application/x-www-form-urlencoded"),
        ("content-length", &*content_length_header),
    ];
    let url = SPACEAPI_SENSOR_ENDPOINT;

    // Send request
    let mut request = client.put(url, &headers)?;
    request.write_all(payload)?;
    request.flush()?;
    log::info!("-> PUT {}", url);
    let response = request.submit()?;

    // Process response
    let status = response.status();
    log::info!("<- {}", status);
    if status == 204 {
        log::info!("Successfully set people now present count to {people_count}");
        Ok(())
    } else {
        anyhow::bail!(format!(
            "Received unexpected HTTP {status} when sending status update"
        ))
    }
}
