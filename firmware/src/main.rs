#![no_main]
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

use cortex_m_semihosting::hprintln;
use embedded_hal::digital::v2::OutputPin;
use rtfm::app;
use stm32f1xx_hal::{prelude::*, pac};
use stm32f1xx_hal::delay::Delay;
use stm32f1xx_hal::gpio::{Output, PushPull, gpiob};

#[app(device = stm32f1::stm32f103, peripherals = true)]
const APP: () = {
    struct Resources {
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
    #[init]
    fn init(ctx: init::Context) -> init::LateResources {
        hprintln!("Initializing").unwrap();

        // Cortex-M peripherals
        let core: cortex_m::Peripherals = ctx.core;

        // Device specific peripherals
        let device: pac::Peripherals = ctx.device;

        // Get reference to peripherals
        let mut rcc = device.RCC.constrain();
        let mut afio = device.AFIO.constrain(&mut rcc.apb2);
        let mut gpioa = device.GPIOA.split(&mut rcc.apb2);
        let mut gpiob = device.GPIOB.split(&mut rcc.apb2);
        let mut flash = device.FLASH.constrain();

        // Disable JTAG to free up pins PA15, PB3 and PB4 for normal use 
        let (_pa15, pb3, pb4) = afio.mapr.disable_jtag(gpioa.pa15, gpiob.pb3, gpiob.pb4);

        // Clock configuration
        let clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(48.mhz())
            .pclk1(24.mhz())
            .freeze(&mut flash.acr);

        // Set up delay provider
        let mut delay = Delay::new(core.SYST, clocks);

        // Set up status LEDs and blink twice
        let mut led_pwr = pb3.into_push_pull_output(&mut gpiob.crl);
        let mut led_wifi = pb4.into_push_pull_output(&mut gpiob.crl);
        let blink_ms: u16 = 100;
        for _ in 0..3 {
            led_pwr.set_high().unwrap();
            delay.delay_ms(blink_ms);
            led_wifi.set_high().unwrap();
            delay.delay_ms(blink_ms);
            led_pwr.set_low().unwrap();
            delay.delay_ms(blink_ms);
            led_wifi.set_low().unwrap();
            delay.delay_ms(blink_ms);
        }

        // Initialize tubes
        let mut tube1_a = gpioa.pa3.into_push_pull_output(&mut gpioa.crl);
        let mut tube1_b = gpioa.pa1.into_push_pull_output(&mut gpioa.crl);
        let mut tube1_c = gpioa.pa0.into_push_pull_output(&mut gpioa.crl);
        let mut tube1_d = gpioa.pa2.into_push_pull_output(&mut gpioa.crl);

        // Show number 3
        tube1_a.set_high().unwrap();
        tube1_b.set_high().unwrap();
        tube1_c.set_low().unwrap();
        tube1_d.set_low().unwrap();

        hprintln!("Init done").unwrap();

        // Assign resources
        init::LateResources {
            led_pwr,
            led_wifi,
        }
    }

    // RTFM requires that free interrupts are declared in an extern block when
    // using software tasks; these free interrupts will be used to dispatch the
    // software tasks.
    extern "C" {
        fn SPI1();
        fn SPI2();
    }
};
