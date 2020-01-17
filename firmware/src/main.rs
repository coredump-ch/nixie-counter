#![no_main]
#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

// pick a panicking behavior
extern crate panic_halt; // you can put a breakpoint on `rust_begin_unwind` to catch panics
// extern crate panic_abort; // requires nightly
// extern crate panic_itm; // logs messages over ITM; requires ITM support
// extern crate panic_semihosting; // logs messages to the host stderr; requires a debugger

mod command;

use core::convert::Infallible;

use at_rs::Error as AtError;
use cortex_m_semihosting::hprintln;
use embedded_hal::digital::v2::{InputPin, OutputPin};
use embedded_hal::serial::{Read, Write};
use heapless::{consts, spsc::Queue};
use rtfm::app;
use rtfm::cyccnt::U32Ext;
use stm32f1xx_hal::{prelude::*, pac};
use stm32f1xx_hal::gpio::{Input, Output, PullUp, PushPull, gpioa, gpiob};
use stm32f1xx_hal::serial::{self, Rx, Serial, Tx};
use stm32f1xx_hal::stm32::USART1;
use stm32f1xx_hal::timer::Timer;

use command::{Command, Response};

// How often (in CPU cycles) the toggle switch should be polled
const POLL_PERIOD: u32 = 9600; // ~0.2ms

// AT command queue capacities
// Note: To get better performance use a capacity that is a power of 2
type AtRxBufferLen = consts::U1024;
type AtCmdQueueLen = consts::U8;
type AtRespQueueLen = consts::U8;

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

        // AT command parser
        at_parser: at_rs::ATParser<SerialTxRx<USART1>, Command, AtRxBufferLen, AtCmdQueueLen, AtRespQueueLen>,
    }

    /// Initialization happens here.
    ///
    /// The init function will run with interrupts disabled and has exclusive
    /// access to Cortex-M and device specific peripherals through the `core`
    /// and `device` variables, which are injected in the scope of init by the
    /// app attribute.
    #[init(spawn = [poll_buttons, at_loop])]
    fn init(ctx: init::Context) -> init::LateResources {
        // ESP8266 AT command queues
        static mut CMD_Q: Option<Queue<Command, AtCmdQueueLen, u8>> = None;
        static mut RESP_Q: Option<Queue<Result<Response, AtError>, AtRespQueueLen, u8>> = None;

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
        let clocks = rcc
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

        // Initialize timer for serial communication
        let timer = Timer::tim2(device.TIM2, &clocks, &mut rcc.apb1);

        // Initialize queues
        *CMD_Q = Some(Queue::u8());
        *RESP_Q = Some(Queue::u8());

        // Set up serial communication with ESP8266 modem
        let mut serial = Serial::usart1(
            device.USART1,
            (
                gpioa.pa9.into_alternate_push_pull(&mut gpioa.crh),
                gpioa.pa10,
            ),
            &mut afio.mapr,
            serial::Config::default().baudrate(115_200.bps()),
            clocks,
            &mut rcc.apb2,
        );
        serial.listen(serial::Event::Rxne);
        let (tx, rx) = serial.split();

        // Initialize AT command client
        let (at_client, at_parser) = at_rs::new(
            (CMD_Q.as_mut().unwrap(), RESP_Q.as_mut().unwrap()),
            SerialTxRx { tx, rx },
            timer.start_count_down(1.hz()),
            1.hz(),
        );
        let (mut at_cmd_producer, _) = at_client.release();

        // Spawn AT loop, enqueue AT command
        ctx.spawn.at_loop().unwrap();
        at_cmd_producer.enqueue(Command::At).expect("AT command cannot be enqueued");

        hprintln!("Init done").unwrap();

        // Assign resources
        init::LateResources {
            btn_up,
            btn_dn,
            led_pwr,
            led_wifi,
            at_parser,
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

    #[task(schedule = [at_loop], resources = [at_parser])]
    fn at_loop(mut ctx: at_loop::Context) {
        ctx.resources.at_parser.lock(|at| at.spin());

        // Adjust this spin rate to set how often the request/response queue is checked
        ctx.schedule
            .at_loop(ctx.scheduled + 1_000_000.cycles())
            .unwrap();
    }

    #[task(binds = USART1, priority = 4, resources = [at_parser])]
    fn serial_irq(ctx: serial_irq::Context) {
        ctx.resources.at_parser.handle_irq();
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

pub struct SerialTxRx<USART> {
    tx: Tx<USART>,
    rx: Rx<USART>,
}

impl Read<u8> for SerialTxRx<USART1> {
    type Error = serial::Error;

    fn read(&mut self) -> nb::Result<u8, serial::Error> {
        self.rx.read()
    }
}

impl Write<u8> for SerialTxRx<USART1> {
    type Error = Infallible;

    fn write(&mut self, word: u8) -> Result<(), nb::Error<Self::Error>> {
        self.tx.write(word)
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        self.tx.flush()
    }
}
