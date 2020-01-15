#![no_main]
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

use cortex_m_semihosting::hprintln;
use embedded_hal::digital::v2::{InputPin, OutputPin};
use rtfm::app;
use rtfm::cyccnt::U32Ext;
use stm32f1xx_hal::{prelude::*, pac};
use stm32f1xx_hal::gpio::{Input, Output, PullUp, PushPull, gpioa, gpiob};

// How often (in CPU cycles) the toggle switch should be polled
const POLL_PERIOD: u32 = 9600; // ~0.2ms

#[app(device = stm32f1::stm32f103, peripherals = true, monotonic = rtfm::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        // Buttons
        btn_up: gpioa::PA11<Input<PullUp>>,
        btn_dn: gpioa::PA8<Input<PullUp>>,

        // LEDs
        led_pwr: gpiob::PB3<Output<PushPull>>,
        led_wifi: gpiob::PB4<Output<PushPull>>,

        // Counter
        #[init(0)]
        people_counter: u8,

        // Debouncing state
        #[init(0)]
        debounce_state_up: u16,
        #[init(0)]
        debounce_state_down: u16,
    }

    /// Initialization happens here.
    ///
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init(spawn = [poll_buttons])]
    fn init(ctx: init::Context) -> init::LateResources {
        hprintln!("Initializing").unwrap();

        // Cortex-M peripherals
        let mut core: rtfm::Peripherals = ctx.core;

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

        // Initialize (enable) the monotonic timer (CYCCNT)
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();

        // Set up toggle inputs
        let btn_up = gpioa.pa11.into_pull_up_input(&mut gpioa.crh);
        let btn_dn = gpioa.pa8.into_pull_up_input(&mut gpioa.crh);

        // Clock configuration
        let _clocks = rcc
            .cfgr
            .use_hse(8.mhz())
            .sysclk(48.mhz())
            .pclk1(24.mhz())
            .freeze(&mut flash.acr);

        // Schedule polling timer for toggle switch
        ctx.spawn.poll_buttons().unwrap();

        // Set up status LEDs and blink twice
        let mut led_pwr = pb3.into_push_pull_output(&mut gpiob.crl);
        let led_wifi = pb4.into_push_pull_output(&mut gpiob.crl);
        led_pwr.set_high().unwrap();

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
            btn_up,
            btn_dn,
            led_pwr,
            led_wifi,
        }
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        hprintln!("Idle").unwrap();

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
        resources = [btn_up, btn_dn, debounce_state_up, debounce_state_down],
        spawn = [pushed_up, pushed_down],
        schedule = [poll_buttons],
    )]
    fn poll_buttons(ctx: poll_buttons::Context) {
        // Specify mask. Only consider the first 12 bits.
        let mask: u16 = 0b0000_1111_1111_1111;

        // Poll GPIOs
        let up_pushed: bool = ctx.resources.btn_up.is_low().unwrap();
        let down_pushed: bool = ctx.resources.btn_dn.is_low().unwrap();

        // Update state
        let up_pushed_debounced = update_state(ctx.resources.debounce_state_up, up_pushed, mask);
        let down_pushed_debounced = update_state(ctx.resources.debounce_state_down, down_pushed, mask);

        // Schedule state change handlers
        if up_pushed_debounced {
            ctx.spawn.pushed_up().unwrap();
        }
        if down_pushed_debounced {
            ctx.spawn.pushed_down().unwrap();
        }

        // Re-schedule the timer interrupt
        ctx.schedule.poll_buttons(ctx.scheduled + POLL_PERIOD.cycles()).unwrap();
    }

    /// The "up" switch was pushed.
    #[task(resources = [people_counter])]
    fn pushed_up(ctx: pushed_up::Context) {
        if *ctx.resources.people_counter < 99 {
            *ctx.resources.people_counter += 1;
        }
        hprintln!("Pushed up ({})", ctx.resources.people_counter).unwrap();
    }

    /// The "down" switch was pushed.
    #[task(resources = [people_counter])]
    fn pushed_down(ctx: pushed_down::Context) {
        if *ctx.resources.people_counter > 0 {
            *ctx.resources.people_counter -= 1;
        }
        hprintln!("Pushed dn ({})", ctx.resources.people_counter).unwrap();
    }

    // RTFM requires that free interrupts are declared in an extern block when
    // using software tasks; these free interrupts will be used to dispatch the
    // software tasks.
    extern "C" {
        fn SPI1();
        fn SPI2();
    }
};

/// Update state by left-shifting in the current button state.
/// Then apply the mask (boolean AND) to limit the number of bits considered.
/// Return true if all masked bits are set, but weren't set previously (rising edge).
#[inline]
fn update_state(state: &mut u16, pushed: bool, mask: u16) -> bool {
    // If all bits are already set and there was no change,
    // we can immediately return false since we're only interested
    // in the rising edge.
    if *state == mask && pushed {
        return false;
    }

    // Update state by shifting in the push state & masking.
    *state = ((*state << 1) | (pushed as u16)) & mask;

    // Return whether all bits are now set
    *state == mask
}
