#![no_main]
#![cfg_attr(not(test), no_std)]
#![allow(clippy::type_complexity)]

use core::sync::atomic::{self, Ordering};

use defmt_rtt as _;

mod timer;

#[rtic::app(
    device = stm32f1::stm32f103,
    peripherals = true,
    dispatchers = [SPI1, SPI2],
)]
mod app {
    use bbqueue::BBBuffer;
    use esp_at_nal::{
        urc::URCMessages as UrcMessages,
        wifi::{self, WifiAdapter},
    };
    use stm32f1xx_hal::{
        pac,
        prelude::*,
        serial::{self, Tx},
    };
    use systick_monotonic::Systick;

    use super::timer::DwtTimer;

    // The main frequency in Hz
    const FREQUENCY: u32 = 48_000_000;

    // Chunk size in bytes when sending data. Higher value results in better
    // performance, but introduces also higher stack memory footprint. Max value: 8192.
    const TX_SIZE: usize = 1024;
    // Chunk size in bytes when receiving data. Value should be matched to buffer
    // size of receive() calls.
    const RX_SIZE: usize = 2048;
    // Constants derived from TX_SIZE and RX_SIZE
    const ESP_TX_SIZE: usize = TX_SIZE;
    const ESP_RX_SIZE: usize = RX_SIZE;
    const ATAT_RX_SIZE: usize = RX_SIZE;
    const URC_RX_SIZE: usize = RX_SIZE;
    const RES_CAPACITY: usize = RX_SIZE;
    const URC_CAPACITY: usize = RX_SIZE * 3;

    #[monotonic(binds = SysTick, default = true)]
    type SystickMonotonic = Systick<1000>;

    type AtatIngress = atat::IngressManager<
        atat::AtDigester<UrcMessages<URC_RX_SIZE>>,
        ATAT_RX_SIZE,
        RES_CAPACITY,
        URC_CAPACITY,
    >;

    type AtatClient<USART> =
        atat::Client<Tx<USART>, DwtTimer<FREQUENCY>, FREQUENCY, RES_CAPACITY, URC_CAPACITY>;

    type EspWifiAdapter<USART> =
        wifi::Adapter<AtatClient<USART>, DwtTimer<FREQUENCY>, FREQUENCY, ESP_TX_SIZE, ESP_RX_SIZE>;

    #[shared]
    struct SharedResources {
        // ATAT ingress manager
        atat_ingress: AtatIngress,
    }

    #[local]
    struct LocalResources {
        // ESP WiFi adapter
        wifi_adapter: EspWifiAdapter<pac::USART1>,
    }

    /// Initialization happens here.
    ///
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init(
        local = [
            res_queue: BBBuffer<RES_CAPACITY> = BBBuffer::new(),
            urc_queue: BBBuffer<URC_CAPACITY> = BBBuffer::new(),
        ]
    )]
    fn init(ctx: init::Context) -> (SharedResources, LocalResources, init::Monotonics) {
        let core: cortex_m::Peripherals = ctx.core;
        let device: pac::Peripherals = ctx.device;
        let rcc = device.RCC.constrain();
        let mut afio = device.AFIO.constrain();
        let mut gpioa = device.GPIOA.split();
        let mut flash = device.FLASH.constrain();

        // Initialize (enable) the monotonic timer
        let mono = Systick::new(core.SYST, FREQUENCY);

        // Clock configuration
        let clocks = rcc
            .cfgr
            .use_hse(8.MHz())
            .sysclk(FREQUENCY.Hz())
            .pclk1(24.MHz())
            .freeze(&mut flash.acr);

        // Initialize UART for ESP8266
        let pin_tx = gpioa.pa9.into_alternate_push_pull(&mut gpioa.crh);
        let pin_rx = gpioa.pa10;
        let serial = serial::Serial::new(
            device.USART1,
            (pin_tx, pin_rx),
            &mut afio.mapr,
            serial::Config::default().baudrate(115_200.bps()),
            &clocks,
        );
        let (esp_tx, _esp_rx) = serial.split();

        // Create static queues for ATAT
        let queues = atat::Queues {
            res_queue: ctx.local.res_queue.try_split_framed().unwrap(),
            urc_queue: ctx.local.urc_queue.try_split_framed().unwrap(),
        };

        // Instantiate ATAT client & ingress manager
        let atat_timer = DwtTimer::<FREQUENCY>::new();
        let config = atat::Config::new(atat::Mode::Blocking);
        let digester = atat::AtDigester::<UrcMessages<URC_RX_SIZE>>::new();
        let (atat_client, mut atat_ingress) =
            atat::ClientBuilder::new(esp_tx, atat_timer, digester, config).build(queues);

        // FAULT: Removing this line gets rid of the hardfault
        atat_ingress.digest();

        // Instantiate ESP AT adapter
        let esp_timer = DwtTimer::<FREQUENCY>::new();
        let wifi_adapter: EspWifiAdapter<_> = wifi::Adapter::new(atat_client, esp_timer);

        // FAULT: Removing this line gets rid of the hardfault
        query_wifi_join_status::spawn().unwrap();

        // Assign resources
        let shared_resources = SharedResources { atat_ingress };
        let local_resources = LocalResources { wifi_adapter };
        (shared_resources, local_resources, init::Monotonics(mono))
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {}
    }

    #[task(local = [wifi_adapter])]
    fn query_wifi_join_status(ctx: query_wifi_join_status::Context) {
        ctx.local.wifi_adapter.get_join_status();
    }

    // FAULT: This task is never called! But removing it get rid of the hardfault.
    #[task(shared = [atat_ingress])]
    fn at_digest_loop(mut ctx: at_digest_loop::Context) {
        ctx.shared.atat_ingress.lock(|ingress| ingress.digest());
    }
}

#[inline(never)]
#[panic_handler]
fn core_panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::interrupt::disable();
    defmt::error!("Panic!");
    loop {
        atomic::compiler_fence(Ordering::SeqCst);
    }
}
