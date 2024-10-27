use esp_idf_svc::hal::{gpio::{PinDriver, Pull}, prelude::Peripherals, task::block_on};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    let mut button = PinDriver::input(peripherals.pins.gpio9)?;
    let mut led = PinDriver::output(peripherals.pins.gpio3)?;

    button.set_pull(Pull::Up)?;

    log::info!("Starting nixie firmware v{VERSION}...");

    block_on(async {
        loop {
            button.wait_for_low().await?;

            log::info!("Pressed");
            led.toggle()?;

            button.wait_for_high().await?;

            log::info!("Released");
        }
    })
}
