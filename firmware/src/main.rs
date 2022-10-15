#![no_main]
#![cfg_attr(not(test), no_std)]
#![allow(clippy::type_complexity)]

use core::sync::atomic::{self, Ordering};

use defmt_rtt as _;

mod nixie;
mod timer;

#[rtic::app(
    device = stm32f1::stm32f103,
    peripherals = true,
    dispatchers = [SPI1, SPI2],
)]
mod app {
    use bbqueue::BBBuffer;
    use cortex_m::asm::delay;
    use debouncr::{debounce_stateful_12, DebouncerStateful, Edge, Repeat12};
    use defmt::unwrap;
    use esp_at_nal::{
        urc::URCMessages as UrcMessages,
        wifi::{self, WifiAdapter},
    };
    use stm32f1xx_hal::{
        gpio::{gpioa, gpiob, Input, Output, PinState, PullUp, PushPull},
        pac,
        prelude::*,
        serial::{self, Rx, Tx},
    };
    use systick_monotonic::{ExtU64, Systick};

    use super::{
        nixie::{NixieTube, NixieTubePair},
        timer::DwtTimer,
    };

    // The main frequency in Hz
    const FREQUENCY: u32 = 48_000_000;

    // How fast (in CPU cycles) the toggle switch should be polled
    const SELFTEST_DELAY: u32 = FREQUENCY / 20; // ~0.05s

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
        // Tubes
        #[lock_free]
        tubes: NixieTubePair<
            gpioa::PA3<Output<PushPull>>,
            gpioa::PA1<Output<PushPull>>,
            gpioa::PA0<Output<PushPull>>,
            gpioa::PA2<Output<PushPull>>,
            gpioa::PA7<Output<PushPull>>,
            gpioa::PA5<Output<PushPull>>,
            gpioa::PA4<Output<PushPull>>,
            gpioa::PA6<Output<PushPull>>,
        >,

        // Counter
        #[lock_free]
        people_counter: u8,

        // ATAT ingress manager
        atat_ingress: AtatIngress,

        // ESP WiFi adapter
        wifi_adapter: EspWifiAdapter<pac::USART1>,
    }

    #[local]
    struct LocalResources {
        // Buttons
        btn_up: gpioa::PA11<Input<PullUp>>,
        btn_dn: gpioa::PA8<Input<PullUp>>,

        // Debouncing state
        debounce_up: DebouncerStateful<u16, Repeat12>,
        debounce_down: DebouncerStateful<u16, Repeat12>,

        // LEDs
        led_pwr: gpiob::PB3<Output<PushPull>>,
        led_wifi: gpiob::PB4<Output<PushPull>>,

        // ESP
        esp_rx: Rx<pac::USART1>,
    }

    /// Initialization happens here.
    ///
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init()]
    fn init(ctx: init::Context) -> (SharedResources, LocalResources, init::Monotonics) {
        defmt::info!("init");

        // Cortex-M peripherals
        let core: cortex_m::Peripherals = ctx.core;

        // Device specific peripherals
        let device: pac::Peripherals = ctx.device;

        // Get reference to peripherals
        let rcc = device.RCC.constrain();
        let mut afio = device.AFIO.constrain();
        let mut gpioa = device.GPIOA.split();
        let mut gpiob = device.GPIOB.split();
        let mut flash = device.FLASH.constrain();

        // Disable JTAG to free up pins PA15, PB3 and PB4 for normal use
        let (_pa15, pb3, pb4) = afio.mapr.disable_jtag(gpioa.pa15, gpiob.pb3, gpiob.pb4);

        // Initialize (enable) the monotonic timer
        let mono = Systick::new(core.SYST, FREQUENCY);

        // Clock configuration
        let clocks = rcc
            .cfgr
            .use_hse(8.MHz())
            .sysclk(FREQUENCY.Hz())
            .pclk1(24.MHz())
            .freeze(&mut flash.acr);

        // Set up toggle inputs
        let btn_up = gpioa.pa11.into_pull_up_input(&mut gpioa.crh);
        let btn_dn = gpioa.pa8.into_pull_up_input(&mut gpioa.crh);

        // Schedule polling timer for toggle switch
        unwrap!(poll_buttons::spawn());

        // Set up status LEDs and blink
        let mut led_pwr = pb3.into_push_pull_output(&mut gpiob.crl);
        let mut led_wifi = pb4.into_push_pull_output(&mut gpiob.crl);
        for _ in 0..2 {
            led_pwr.set_high();
            led_wifi.set_high();
            delay(SELFTEST_DELAY);
            led_pwr.set_low();
            led_wifi.set_low();
            delay(SELFTEST_DELAY);
        }
        led_pwr.set_high();

        // Initialize tubes
        let mut tubes = NixieTubePair::new(
            NixieTube {
                pin_a: gpioa.pa3.into_push_pull_output(&mut gpioa.crl),
                pin_b: gpioa.pa1.into_push_pull_output(&mut gpioa.crl),
                pin_c: gpioa.pa0.into_push_pull_output(&mut gpioa.crl),
                pin_d: gpioa.pa2.into_push_pull_output(&mut gpioa.crl),
            },
            NixieTube {
                pin_a: gpioa.pa7.into_push_pull_output(&mut gpioa.crl),
                pin_b: gpioa.pa5.into_push_pull_output(&mut gpioa.crl),
                pin_c: gpioa.pa4.into_push_pull_output(&mut gpioa.crl),
                pin_d: gpioa.pa6.into_push_pull_output(&mut gpioa.crl),
            },
        );
        // Tubes self-test
        for i in 0..=9 {
            tubes.left().show_digit(i);
            tubes.right().show_digit(i);
            delay(SELFTEST_DELAY);
        }
        tubes.off();

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
        let (esp_tx, esp_rx) = serial.split();

        // Create static queues for ATAT
        static mut RES_QUEUE: BBBuffer<RES_CAPACITY> = BBBuffer::new();
        static mut URC_QUEUE: BBBuffer<URC_CAPACITY> = BBBuffer::new();
        let queues = atat::Queues {
            res_queue: unsafe { RES_QUEUE.try_split_framed().unwrap() },
            urc_queue: unsafe { URC_QUEUE.try_split_framed().unwrap() },
        };

        // Instantiate ATAT client & ingress manager
        let atat_timer = DwtTimer::<FREQUENCY>::new();
        let config = atat::Config::new(atat::Mode::Blocking);
        let digester = atat::AtDigester::<UrcMessages<URC_RX_SIZE>>::new();
        let (atat_client, mut atat_ingress) =
            atat::ClientBuilder::new(esp_tx, atat_timer, digester, config).build(queues);

        defmt::info!("digest1");
        atat_ingress.digest();
        defmt::info!("digest2");
        atat_ingress.digest();
        defmt::info!("digest3");
        atat_ingress.digest();
        defmt::info!("digest done");

        // Spawn ATAT digest loop
        at_digest_loop::spawn().ok();

        // Instantiate ESP AT adapter
        // TODO: I'm not sure if it's safe to re-use DwtTimer!
        let esp_timer = DwtTimer::<FREQUENCY>::new();
        let wifi_adapter: EspWifiAdapter<_> = wifi::Adapter::new(atat_client, esp_timer);

        // Spawn WiFi status loop and join tasks
//        wifi_status_loop::spawn().ok();
//        wifi_join::spawn().ok();

        // Assign resources
        let shared_resources = SharedResources {
            tubes,
            people_counter: 0,
            atat_ingress,
            wifi_adapter,
        };
        let local_resources = LocalResources {
            btn_up,
            btn_dn,
            debounce_up: debounce_stateful_12(false),
            debounce_down: debounce_stateful_12(false),
            led_pwr,
            led_wifi,
            esp_rx,
        };
        (shared_resources, local_resources, init::Monotonics(mono))
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        // The idle loop
        loop {}
    }

    /// Regularly called task that polls the buttons and debounces them.
    ///
    /// The handlers are only called for a rising edge with 12 consecutive high
    /// pin inputs. This means that if the interrupt is scheduled every 1 ms
    /// and the input pin becomes high, the task will fire after 12 ms. Every
    /// low input will reset the whole state though.
    #[task(
        local = [btn_up, btn_dn, debounce_up, debounce_down],
    )]
    fn poll_buttons(ctx: poll_buttons::Context) {
        // Poll GPIOs
        let up_pushed: bool = ctx.local.btn_up.is_low();
        let down_pushed: bool = ctx.local.btn_dn.is_low();

        // Update state
        let up_edge = ctx.local.debounce_up.update(up_pushed);
        let down_edge = ctx.local.debounce_down.update(down_pushed);

        // Schedule state change handlers
        if up_edge == Some(Edge::Rising) {
            unwrap!(pushed_up::spawn());
        }
        if down_edge == Some(Edge::Rising) {
            unwrap!(pushed_down::spawn());
        }

        // Re-schedule the timer interrupt every 200µs
        unwrap!(poll_buttons::spawn_at(monotonics::now() + ExtU64::micros(200)));
    }

    /// The "up" switch was pushed.
    #[task(shared = [people_counter, tubes])]
    fn pushed_up(ctx: pushed_up::Context) {
        if *ctx.shared.people_counter < 99 {
            *ctx.shared.people_counter += 1;
        }
        ctx.shared.tubes.show(*ctx.shared.people_counter);
    }

    /// The "down" switch was pushed.
    #[task(shared = [people_counter, tubes])]
    fn pushed_down(ctx: pushed_down::Context) {
        if *ctx.shared.people_counter > 0 {
            *ctx.shared.people_counter -= 1;
        }
        ctx.shared.tubes.show(*ctx.shared.people_counter);
    }

    #[task(shared = [wifi_adapter], local = [led_wifi])]
    fn wifi_status_loop(mut ctx: wifi_status_loop::Context) {
        defmt::trace!("wifi_status_loop");

        // Check join state, update LED
        ctx.shared.wifi_adapter.lock(|adapter| {
            // Turn on WiFi LED if we're connected and have an IP assigned
            let join_state = adapter.get_join_status();
            let connected = join_state.connected && join_state.ip_assigned;
            ctx.local.led_wifi.set_state(PinState::from(!connected));
        });

        // Re-schedule WiFi status check every 1s
        wifi_status_loop::spawn_at(monotonics::now() + ExtU64::millis(1000)).ok();
    }

    #[task(shared = [wifi_adapter])]
    fn wifi_join(ctx: wifi_join::Context) {
        defmt::info!("wifi_join");
        //ctx.shared.wifi_adapter.lock(|adapter| {
        //    adapter.join(
        //});
    }

    #[task(shared = [atat_ingress], priority = 2)]
    fn at_digest_loop(mut ctx: at_digest_loop::Context) {
        defmt::trace!("at_digest_loop");

        ctx.shared.atat_ingress.lock(|ingress| ingress.digest());

        // Re-schedule checking of request/response queue every 100ms
        at_digest_loop::spawn_at(monotonics::now() + ExtU64::millis(1000 /* TODO revert to 100 */)).ok();
    }

//    /// Task that handles serial interrupts and forwards the byte to the atat
//    /// ingress.
//    #[task(binds = USART1, priority = 3, shared = [atat_ingress], local = [esp_rx])]
//    fn serial_irq(mut ctx: serial_irq::Context) {
//        let rx = ctx.local.esp_rx;
//        ctx.shared.atat_ingress.lock(|ingress| {
//            if let Ok(byte) = rx.read() {
//                ingress.write(&[byte]);
//            }
//        });
//    }
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

#[defmt::panic_handler]
fn defmt_panic() -> ! {
    loop {
        atomic::compiler_fence(Ordering::SeqCst);
    }
}
