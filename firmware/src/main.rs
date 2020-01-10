#![no_main]
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

use embedded_hal::digital::v2::OutputPin;
use rtfm::app;
use stm32f1xx_hal::{prelude::*, pac};
use stm32f1xx_hal::delay::Delay;
use stm32f1xx_hal::gpio::{Output, PushPull, gpiob};

#[app(device = stm32f1::stm32f103)]
const APP: () = {
    // LEDs
    static mut LED_PWR: gpiob::PB3<Output<PushPull>> = ();
    static mut LED_WIFI: gpiob::PB4<Output<PushPull>> = ();

    /// Initialization happens here.
    /// 
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init]
    fn init() {
        // Cortex-M peripherals
        let core: rtfm::Peripherals = core;

        // Device specific peripherals
        let device: pac::Peripherals = device;

        // Get reference to peripherals
        let mut rcc = device.RCC.constrain();
        let mut afio = device.AFIO.constrain(&mut rcc.apb2);
        let gpioa = device.GPIOA.split(&mut rcc.apb2);
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

        // Assign resources
        LED_PWR = led_pwr;
        LED_WIFI = led_wifi;
    }

    // RTFM requires that free interrupts are declared in an extern block when
    // using software tasks; these free interrupts will be used to dispatch the
    // software tasks.
    extern "C" {
        fn SPI1();
        fn SPI2();
    }
};
