#![no_std]
#![no_main]

use core::{fmt::Write, str::FromStr};

use embassy_executor::Spawner;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    DhcpConfig, Stack, StackResources,
};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Receiver, Sender},
};
use embassy_time::{Duration, Timer};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output},
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_wifi::{
    wifi::{
        ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice,
        WifiState,
    },
    EspWifiController,
};
use reqwless::{
    client::HttpClient,
    request::{Method, RequestBuilder},
    response::Status,
};
use toggle_switch::Direction;

mod nixie;
mod toggle_switch;

use crate::{
    nixie::{NixieTube, NixieTubePair},
    toggle_switch::ToggleSwitch,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const WIFI_SSID: &str = env!("WIFI_SSID");
const WIFI_PASS: &str = env!("WIFI_PASS");
const SPACEAPI_SENSOR_ENDPOINT: &str = env!("SPACEAPI_SENSOR_ENDPOINT");

const DHCP_HOSTNAME: &str = "Nixie Counter";

type EspWifiDevice<'a> = WifiDevice<'a, WifiStaDevice>;
type EspTcpClient<'a> = TcpClient<'a, EspWifiDevice<'a>, 1>;
type EspDnsSocket<'a> = DnsSocket<'a, EspWifiDevice<'a>>;
type EspHttpClient<'a> = HttpClient<'a, EspTcpClient<'a>, EspDnsSocket<'a>>;

// Note: When you are okay with using a nightly compiler it's better to
// use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
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
    let mut toggle_switch = ToggleSwitch::new(peripherals.GPIO1, peripherals.GPIO0);

    // Set up LEDs
    let _led_pwr = Output::new(peripherals.GPIO20, Level::High);
    let led_wifi = Output::new(peripherals.GPIO21, Level::Low);

    // Initialize tubes
    let mut tubes = NixieTubePair::new(
        NixieTube {
            pin_a: Output::new(peripherals.GPIO6, Level::Low),
            pin_b: Output::new(peripherals.GPIO4, Level::Low),
            pin_c: Output::new(peripherals.GPIO3, Level::Low),
            pin_d: Output::new(peripherals.GPIO5, Level::Low),
        },
        NixieTube {
            pin_a: Output::new(peripherals.GPIO9, Level::Low),
            pin_b: Output::new(peripherals.GPIO8, Level::Low),
            pin_c: Output::new(peripherals.GPIO7, Level::Low),
            pin_d: Output::new(peripherals.GPIO10, Level::Low),
        },
    );
    tubes.selftest(Duration::from_millis(100)).await;

    // Initialize WiFi
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let wifi_init = &*mk_static!(
        EspWifiController<'static>,
        esp_wifi::init(timg1.timer0, rng, peripherals.RADIO_CLK,).expect("Failed to init esp_wifi")
    );
    let (wifi_interface, wifi_controller) =
        esp_wifi::wifi::new_with_mode(wifi_init, peripherals.WIFI, WifiStaDevice).unwrap();
    let wifi_config = ClientConfiguration {
        ssid: WIFI_SSID.try_into().unwrap(),
        password: WIFI_PASS.try_into().unwrap(),
        ..Default::default()
    };

    // Init network stack
    let dhcp_config = {
        let mut config = DhcpConfig::default();
        config.hostname = Some(
            heapless::String::from_str(DHCP_HOSTNAME)
                .expect("Failed to construct heapless string for DHCP hostname"),
        );
        config
    };
    let config = embassy_net::Config::dhcpv4(dhcp_config);
    let seed: u64 = rng.random().into();
    log::debug!("Network stack seed: {seed}");
    let stack = &*mk_static!(
        Stack<WifiDevice<'_, WifiStaDevice>>,
        Stack::new(
            wifi_interface,
            config,
            mk_static!(StackResources<3>, StackResources::<3>::new()),
            seed
        )
    );

    // Spawn LED control task
    let led_control_channel = mk_static!(
        Channel::<NoopRawMutex, LedControlCommand, 3>,
        Channel::<NoopRawMutex, LedControlCommand, 3>::new()
    );
    spawner.must_spawn(led_control_task(led_wifi, led_control_channel.receiver()));

    // Spawn connection tasks
    spawner.must_spawn(connection(
        wifi_controller,
        wifi_config,
        led_control_channel.sender(),
    ));
    spawner.must_spawn(net_task(stack));

    // Wait for link
    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(200)).await;
    }

    // Wait for IP
    log::info!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            log::info!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(200)).await;
    }

    // Create HTTP client (without TLS support for now)
    let client_state = &*mk_static!(
        TcpClientState<1, 1024, 1024>,
        TcpClientState::<1, 1024, 1024>::new()
    );
    let tcp_client = &*mk_static!(
        TcpClient<'static, EspWifiDevice<'static>, 1>,
        TcpClient::new(stack, client_state)
    );
    let dns = &*mk_static!(EspDnsSocket<'_>, DnsSocket::new(stack));
    let mut http_client = HttpClient::new(tcp_client, dns);

    // Send initial count
    match update_people_now_present(&mut http_client, 0).await {
        Ok(()) => log::info!("Sent initial count 0"),
        Err(e) => log::warn!(
            "Failed to update SpaceAPI endpoint with initial value: {}",
            e
        ),
    }

    // Main loop
    let mut count = 0u8;
    log::info!("Starting main loop");
    loop {
        // Wait for toggle switch press
        let direction = toggle_switch.wait_for_press().await;
        log::info!("Pressed {:?}", direction);

        // Debouncing
        Timer::after(Duration::from_millis(250)).await;

        // Update SpaceAPI
        let new_count = match direction {
            Direction::Up => count.saturating_add(1),
            Direction::Down => count.saturating_sub(1),
        };
        match update_people_now_present(&mut http_client, new_count).await {
            Ok(()) => {
                // Success, update nixie tubes
                tubes.show(new_count.min(99));
                count = new_count
            }
            Err(e) => {
                // Failed to update SpaceAPI
                log::error!("Failed to update SpaceAPI endpoint: {}", e)
            }
        }
        // Wait for toggle switch release
        toggle_switch.wait_for_release().await;
    }
}

enum LedControlCommand {
    TurnOn,
    TurnOff,
    Blink { delay: Duration },
}

/// Task: Control WiFi LEDs
#[embassy_executor::task]
async fn led_control_task(
    mut led: Output<'static>,
    command_receiver: Receiver<'static, NoopRawMutex, LedControlCommand, 3>,
) {
    log::info!("Start LED connection task");
    led.set_low();
    loop {
        match command_receiver.receive().await {
            LedControlCommand::TurnOn => led.set_high(),
            LedControlCommand::TurnOff => led.set_low(),
            LedControlCommand::Blink { delay } => 'blink: loop {
                led.toggle();
                if command_receiver.is_empty() {
                    // No new command arrives, keep on blinking
                    Timer::after(delay).await;
                } else {
                    // Otherwise, stop and process the command
                    break 'blink;
                }
            },
        }
    }
}

/// Task: Ensure WiFi connection
#[embassy_executor::task]
async fn connection(
    mut controller: WifiController<'static>,
    config: ClientConfiguration,
    led_command_sender: Sender<'static, NoopRawMutex, LedControlCommand, 3>,
) {
    log::info!("Start connection task");
    let mut previously_connected = false;
    loop {
        // When currently connected, wait until we're no longer connected
        #[allow(clippy::single_match)]
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                led_command_sender.send(LedControlCommand::TurnOff).await;
                if previously_connected {
                    log::info!("WiFi connection lost");
                }
                Timer::after(Duration::from_millis(5000)).await;
            }
            _ => {}
        }

        // Blink LED to indicate connecting status
        led_command_sender
            .send(LedControlCommand::Blink {
                delay: Duration::from_millis(250),
            })
            .await;

        // Start WiFi
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(config.clone());
            controller.set_configuration(&client_config).unwrap();
            log::info!("Starting WiFi");
            controller.start_async().await.unwrap();
            log::info!("WiFi started!");
        }

        // Connect WiFi
        log::info!("About to connect to WiFi \"{}\"...", config.ssid);
        match controller.connect_async().await {
            Ok(_) => {
                log::info!("WiFi \"{}\" connected!", config.ssid);
                led_command_sender.send(LedControlCommand::TurnOn).await;
                previously_connected = true;
            }
            Err(e) => {
                log::info!("Failed to connect to WiFi: {e:?}");
                Timer::after(Duration::from_millis(2000)).await
            }
        }
    }
}

/// Task: Run network stack
#[embassy_executor::task]
async fn net_task(stack: &'static Stack<EspWifiDevice<'static>>) {
    stack.run().await
}

/// Update the "people now present" sensor through HTTP.
async fn update_people_now_present<'a>(
    client: &mut EspHttpClient<'a>,
    people_count: u8,
) -> anyhow::Result<()> {
    // Prepare URL and payload
    let url = SPACEAPI_SENSOR_ENDPOINT;
    let mut payload_string = heapless::String::<9>::new();
    write!(payload_string, "value={people_count}")?;
    let payload = payload_string.as_bytes();

    // Send request
    let mut rx_buf = [0; 4096];
    let request_handle = match client.request(Method::PUT, url).await {
        Ok(handle) => handle,
        Err(e) => {
            log::error!("Could not create HTTP request handle: {:?}", e);
            anyhow::bail!("HTTP request failed");
        }
    };
    let mut request = request_handle
        .headers(&[("content-type", "application/x-www-form-urlencoded")])
        .body(payload);
    log::info!("-> PUT {}", url);
    let response = match request.send(&mut rx_buf).await {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("HTTP request error: {:?}", e);
            anyhow::bail!("HTTP request failed");
        }
    };

    // Process response
    log::info!("<- HTTP {}", response.status.0);
    if response.status == Status::NoContent {
        log::info!("Successfully set people now present count to {people_count}");
        Ok(())
    } else {
        anyhow::bail!("Received unexpected HTTP status code when sending status update")
    }
}
