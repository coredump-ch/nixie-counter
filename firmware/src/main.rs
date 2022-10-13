#![no_main]
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]
#![allow(clippy::type_complexity)]

use panic_rtt_target as _;

mod nixie;

// The main frequency in Hz
const FREQUENCY: u32 = 48_000_000;

// How fast (in CPU cycles) the toggle switch should be polled
const SELFTEST_DELAY: u32 = FREQUENCY / 10; // ~0.1s

#[rtic::app(
    device = stm32f1::stm32f103,
    peripherals = true,
    dispatchers = [SPI1, SPI2],
)]
mod app {
    use cortex_m::asm::delay;
    use debouncr::{debounce_stateful_12, DebouncerStateful, Edge, Repeat12};
    use rtt_target::{rprintln, rtt_init_print};
    use stm32f1xx_hal::{
        gpio::{gpioa, gpiob, Input, Output, PullUp, PushPull},
        pac,
        prelude::*,
    };
    use systick_monotonic::{ExtU64, Systick};

    use super::nixie::{NixieTube, NixieTubePair};

    #[monotonic(binds = SysTick, default = true)]
    type SystickMonotonic = Systick<1000>;

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
    }

    /// Initialization happens here.
    ///
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init()]
    fn init(ctx: init::Context) -> (SharedResources, LocalResources, init::Monotonics) {
        rtt_init_print!();
        rprintln!("init");

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
        let mono = Systick::new(core.SYST, 48_000_000);

        // Clock configuration
        let _clocks = rcc
            .cfgr
            .use_hse(8.MHz())
            .sysclk(super::FREQUENCY.Hz())
            .pclk1(24.MHz())
            .freeze(&mut flash.acr);

        // Set up toggle inputs
        let btn_up = gpioa.pa11.into_pull_up_input(&mut gpioa.crh);
        let btn_dn = gpioa.pa8.into_pull_up_input(&mut gpioa.crh);

        // Schedule polling timer for toggle switch
        poll_buttons::spawn().unwrap();

        // Set up status LEDs and blink
        let mut led_pwr = pb3.into_push_pull_output(&mut gpiob.crl);
        let mut led_wifi = pb4.into_push_pull_output(&mut gpiob.crl);
        for _ in 0..2 {
            led_pwr.set_high();
            led_wifi.set_high();
            delay(super::SELFTEST_DELAY);
            led_pwr.set_low();
            led_wifi.set_low();
            delay(super::SELFTEST_DELAY);
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
        for i in 0..=9 {
            tubes.left().show_digit(i);
            tubes.right().show_digit(i);
            delay(super::SELFTEST_DELAY);
        }
        tubes.off();

        // Assign resources
        let shared_resources = SharedResources {
            tubes,
            people_counter: 0,
        };
        let local_resources = LocalResources {
            btn_up,
            btn_dn,
            debounce_up: debounce_stateful_12(false),
            debounce_down: debounce_stateful_12(false),
            led_pwr,
            led_wifi,
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
        rprintln!("poll_buttons");
        // Poll GPIOs
        let up_pushed: bool = ctx.local.btn_up.is_low();
        let down_pushed: bool = ctx.local.btn_dn.is_low();

        // Update state
        let up_edge = ctx.local.debounce_up.update(up_pushed);
        let down_edge = ctx.local.debounce_down.update(down_pushed);

        // Schedule state change handlers
        if up_edge == Some(Edge::Rising) {
            pushed_up::spawn().unwrap();
        }
        if down_edge == Some(Edge::Rising) {
            pushed_down::spawn().unwrap();
        }

        // Re-schedule the timer interrupt every 200ms
        poll_buttons::spawn_at(monotonics::now() + ExtU64::millis(200)).unwrap();
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
}
