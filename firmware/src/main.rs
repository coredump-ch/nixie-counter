#![no_std]
#![no_main]

use embassy_executor::{task, Spawner};
use embassy_net::{tcp::TcpSocket, Ipv4Address, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Level, Output},
    peripherals,
    prelude::*,
    time,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_wifi::wifi::{
    utils::create_network_interface, AccessPointInfo, ClientConfiguration, Configuration,
    WifiController, WifiError, WifiStaDevice,
};

mod toggle_switch;

use smoltcp::{
    iface::{SocketSet, SocketStorage},
    wire::DhcpOption,
};
use toggle_switch::ToggleSwitch;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const WIFI_SSID: &str = env!("WIFI_SSID");
const WIFI_PASS: &str = env!("WIFI_PASS");
const SPACEAPI_SENSOR_ENDPOINT: &str = env!("SPACEAPI_SENSOR_ENDPOINT");

const WIFI_CONNECT_TIMEOUT_S: u64 = 30;

#[task]
async fn run() {
    loop {
        esp_println::println!("Hello world from embassy using esp-hal-async!");
        Timer::after(Duration::from_millis(1_000)).await;
    }
}

#[task]
async fn error_blink(mut led1: Output<'static>, mut led2: Output<'static>) {
    esp_println::println!("Critical error");
    led1.set_low();
    led2.set_low();
    loop {
        Timer::after(Duration::from_millis(200)).await;
        led1.toggle();
        led2.toggle();
    }
}

/// Toggle LED every 200ms to indicate a critical error. Never return.
async fn critical_error(spawner: &Spawner, led1: Output<'static>, led2: Output<'static>) -> ! {
    spawner.spawn(error_blink(led1, led2)).ok();
    loop {
        Timer::after(Duration::from_millis(1_000)).await;
    }
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // Initialize 72 KiB heap for alloc
    esp_alloc::heap_allocator!(72 * 1024);

    // Initialize logging
    println!("--- start of main() ---");
    esp_println::logger::init_logger(log::LevelFilter::Debug);

    // Initialize peripherals
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut rng = esp_hal::rng::Rng::new(peripherals.RNG);

    // Initialize timer
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    log::info!("Starting nixie firmware v{VERSION}...");

    // Set up toggle switch
    let mut toggle_switch = ToggleSwitch::new(peripherals.GPIO0, peripherals.GPIO1);

    // Set up LEDs
    let mut led_pwr = Output::new(peripherals.GPIO20, Level::High);
    let mut led_wifi = Output::new(peripherals.GPIO21, Level::Low);

    // Spawn test task
    spawner.spawn(run()).ok();

    // Init WiFi
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let wifi_init = esp_wifi::init(timg1.timer0, rng.clone(), peripherals.RADIO_CLK)
        .expect("Failed to init esp_wifi");
    let (wifi_interface, mut controller) =
        esp_wifi::wifi::new_with_mode(&wifi_init, peripherals.WIFI, WifiStaDevice).unwrap();

    // Prepare DHCP socket
    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let mut socket_set = SocketSet::new(&mut socket_set_entries[..]);
    let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
    dhcp_socket.set_outgoing_options(&[DhcpOption {
        kind: 12,
        data: b"Nixie Counter",
    }]);
    socket_set.add(dhcp_socket);

    // Set WiFi configuration
    //let now = || time::now().duration_since_epoch().to_millis();
    //let stack = Stack::new(iface, device, socket_set, now, rng.random());
    let client_config = Configuration::Client(ClientConfiguration {
        ssid: WIFI_SSID.try_into().unwrap(),
        password: WIFI_PASS.try_into().unwrap(),
        ..Default::default()
    });
    let res = controller.set_configuration(&client_config);
    log::debug!("wifi_set_configuration returned {:?}", res);

    // Connect to WiFi
    loop {
        match wifi_connect(&mut controller, &mut led_wifi).await {
            Ok(()) => break,
            Err(e) => {
                log::error!("Connecting to WiFi failed: {e:?}");
                controller.stop_async().await.ok();
                Timer::after(Duration::from_millis(5_00)).await;
            }
        }
    }

    // Main loop
    let mut count = 0usize;
    log::info!("Starting main loop");
    loop {
        log::info!("Bling");
        Timer::after(Duration::from_millis(5_000)).await;
    }

    //    let peripherals = Peripherals::take()?;
    //    let sys_loop = EspSystemEventLoop::take()?;
    //    let timer_service = EspTaskTimerService::new()?;
    //    let nvs = EspDefaultNvsPartition::take()?;
    //
    //    // Initialize tubes
    //    let mut tubes = NixieTubePair::new(
    //        NixieTube {
    //            pin_a: PinDriver::output(peripherals.pins.gpio6.downgrade_output())?,
    //            pin_b: PinDriver::output(peripherals.pins.gpio4.downgrade_output())?,
    //            pin_c: PinDriver::output(peripherals.pins.gpio3.downgrade_output())?,
    //            pin_d: PinDriver::output(peripherals.pins.gpio5.downgrade_output())?,
    //        },
    //        NixieTube {
    //            pin_a: PinDriver::output(peripherals.pins.gpio9.downgrade_output())?,
    //            pin_b: PinDriver::output(peripherals.pins.gpio8.downgrade_output())?,
    //            pin_c: PinDriver::output(peripherals.pins.gpio7.downgrade_output())?,
    //            pin_d: PinDriver::output(peripherals.pins.gpio10.downgrade_output())?,
    //        },
    //    );
    //    block_on(async {
    //        tubes.selftest(&mut timer, Duration::from_millis(100)).await;
    //    });
    //
    //    // Connect WiFi
    //    let mut wifi = AsyncWifi::wrap(
    //        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))
    //            .context("Failed to instantiate EspWifi")?,
    //        sys_loop,
    //        timer_service,
    //    )?;
    //    block_on(connect_wifi(&mut wifi))?;
    //    led_wifi.set_high().context("Could not enable WiFi LED")?;
    //
    //    // Create HTTP client (without TLS support for now)
    //    let mut client = HttpClient::wrap(EspHttpConnection::new(&Default::default())?);
    //
    //    // Main loop
    //    let mut count = 0usize;
    //    log::info!("Starting main loop");
    //    block_on(async {
    //        loop {
    //            // Future 1: Wait for toggle switch press
    //            let toggle_switch_future = toggle_switch.wait_for_press();
    //
    //            // Future 2: WiFi disconnect
    //            let wifi_disconnect_future = wifi.wifi_wait(|wifi| wifi.is_connected(), None);
    //
    //            // Wait for toggle switch or connection loss
    //            let direction = match select(toggle_switch_future, wifi_disconnect_future).await {
    //                // Toggle switch pressed, return direction and carry on
    //                Either::First(direction) => direction,
    //                // WiFi connection lost, reset module
    //                Either::Second(Ok(())) => {
    //                    log::error!("WiFi disconnected, reconnecting");
    //                    led_wifi.set_low().context("Could not disable WiFi LED")?;
    //                    connect_wifi(&mut wifi)
    //                        .await
    //                        .context("Error while reconnecting to WiFi")?;
    //                    led_wifi.set_high().context("Could not enable WiFi LED")?;
    //                    continue;
    //                }
    //                // Error, restart loop
    //                Either::Second(Err(e)) => {
    //                    log::error!("Error while waiting for wifi disconnect: {}", e);
    //                    continue;
    //                }
    //            };
    //            log::info!("Pressed {:?}", direction);
    //
    //            // Debouncing
    //            if let Err(e) = timer.after(Duration::from_millis(100)).await {
    //                log::error!("Failed to wait for debouncing delay: {e}")
    //            }
    //
    //            // Update SpaceAPI
    //            match direction {
    //                Direction::Up => count = count.saturating_add(1),
    //                Direction::Down => count = count.saturating_sub(1),
    //            }
    //            match update_people_now_present(&mut client, count) {
    //                Ok(()) => {
    //                    // Success, update nixie tubes
    //                    tubes.show(
    //                        count
    //                            .min(99)
    //                            .try_into()
    //                            .expect("Failed to convert count to u8"),
    //                    );
    //                }
    //                Err(e) => {
    //                    // Failed to update SpaceAPI
    //                    log::error!("Failed to update SpaceAPI endpoint: {}", e);
    //                }
    //            }
    //
    //            // Wait for toggle switch release
    //            toggle_switch
    //                .wait_for_release()
    //                .await
    //                .context("Failed to wait for toggle release")?;
    //        }
    //    })
}

/// Connect to WiFi
async fn wifi_connect(
    controller: &mut WifiController<'_>,
    led_wifi: &mut Output<'_>,
) -> anyhow::Result<()> {
    // Start WiFi controller
    controller.start_async().await.unwrap();
    match controller.is_started() {
        Ok(true) => {
            log::debug!("WiFi controller started")
        }
        Ok(false) => {
            anyhow::bail!("WiFi controller not started");
        }
        Err(e) => {
            anyhow::bail!("WiFi controller not started: {e:?}");
        }
    }

    // Scan for WiFi networks
    log::info!("Start Wifi Scan");
    let res = controller.scan_n::<10>();
    if let Ok((res, count)) = res {
        log::info!("Found {count} networks");
        for ap in res {
            log::debug!("{:?}", ap);
        }
    }

    // Connect to WiFi
    log::info!("Connecting to WiFi");
    if let Err(e) = controller.connect() {
        anyhow::bail!("Failed to connect to WiFi: {e:?}");
    }
    log::info!("Wait until WiFi connection is established...");
    loop {
        match controller.is_connected() {
            Ok(true) => break,
            Ok(false) => {
                led_wifi.set_high();
                Timer::after(Duration::from_millis(50)).await;
                led_wifi.set_low();
                Timer::after(Duration::from_millis(450)).await;
            }
            Err(e) => {
                anyhow::bail!("Failed to check for WiFi connection: {e:?}");
            }
        }
    }
    log::info!("WiFi connected");
    led_wifi.set_high();

    Ok(())
}

///// Connect to WiFi
/////
///// Credentials must be passed in through env variables (WIFI_SSID and WIFI_PASS).
//async fn connect_wifi(wifi: &mut AsyncWifi<EspWifi<'static>>) -> anyhow::Result<()> {
//    let wifi_configuration: Configuration = Configuration::Client(ClientConfiguration {
//        ssid: WIFI_SSID.try_into().unwrap(),
//        bssid: None,
//        auth_method: AuthMethod::WPA2Personal,
//        password: WIFI_PASS.try_into().unwrap(),
//        channel: None,
//        ..Default::default()
//    });
//
//    wifi.set_configuration(&wifi_configuration)?;
//
//    wifi.start().await.context("Failed to start WiFi")?;
//    log::info!("WiFi started");
//
//    wifi.connect().await.context("Failed to connect WiFi")?;
//    log::info!("WiFi connecting");
//
//    wifi.wait_netif_up()
//        .await
//        .context("Failed to wait for WiFi netif up")?;
//    log::info!("WiFi netif up");
//
//    // Wait while WiFi is not connected
//    wifi.wifi_wait(
//        |wifi| wifi.is_connected().map(|connected| !connected),
//        Some(Duration::from_secs(WIFI_CONNECT_TIMEOUT_S)),
//    )
//    .await
//    .context("Failed to wait for WiFi connected")?;
//    log::info!("WiFi connected");
//
//    Ok(())
//}
//
///// Update the "people now present" sensor through HTTP.
//fn update_people_now_present(
//    client: &mut HttpClient<EspHttpConnection>,
//    people_count: usize,
//) -> anyhow::Result<()> {
//    // Prepare payload
//    let payload_string = format!("value={people_count}");
//    let payload = payload_string.as_bytes();
//
//    // Prepare headers and URL
//    let content_length_header = format!("{}", payload.len());
//    let headers = [
//        ("content-type", "application/x-www-form-urlencoded"),
//        ("content-length", &*content_length_header),
//    ];
//    let url = SPACEAPI_SENSOR_ENDPOINT;
//
//    // Send request
//    let mut request = client.put(url, &headers)?;
//    request.write_all(payload)?;
//    request.flush()?;
//    log::info!("-> PUT {}", url);
//    let response = request.submit()?;
//
//    // Process response
//    let status = response.status();
//    log::info!("<- {}", status);
//    if status == 204 {
//        log::info!("Successfully set people now present count to {people_count}");
//        Ok(())
//    } else {
//        anyhow::bail!(format!(
//            "Received unexpected HTTP {status} when sending status update"
//        ))
//    }
//}
