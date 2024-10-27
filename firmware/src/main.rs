use embedded_svc::{
    http::client::Client as HttpClient,
    wifi::{AuthMethod, ClientConfiguration, Configuration},
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        gpio::{PinDriver, Pull},
        prelude::Peripherals,
        task::block_on,
    },
    http::client::EspHttpConnection,
    io::Write,
    nvs::EspDefaultNvsPartition,
    timer::EspTaskTimerService,
    wifi::{AsyncWifi, EspWifi},
};

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

    // Set up I/O pins
    let mut button = PinDriver::input(peripherals.pins.gpio9)?;
    button.set_pull(Pull::Up)?;
    let mut led = PinDriver::output(peripherals.pins.gpio3)?;

    // Connect WiFi
    let mut wifi = AsyncWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
        timer_service,
    )?;
    block_on(connect_wifi(&mut wifi))?;

    // Create HTTP client (without TLS support for now)
    let mut client = HttpClient::wrap(EspHttpConnection::new(&Default::default())?);

    // Main loop
    let mut count = 0usize;
    block_on(async {
        loop {
            button.wait_for_low().await?;

            log::info!("Pressed");
            led.toggle()?;

            count += 1;
            update_people_now_present(&mut client, count)?;

            button.wait_for_high().await?;

            log::info!("Released");
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
