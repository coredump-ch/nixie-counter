use embassy_time::{Duration, Timer};
use embedded_hal::digital::OutputPin;

/// A nixie tube.
///
/// The struct needs to be initialized with the four output pins connected to
/// the K155ID1 BCD encoder.
pub struct NixieTube<A, B, C, D> {
    pub pin_a: A,
    pub pin_b: B,
    pub pin_c: C,
    pub pin_d: D,
}

/// A pair of two nixie tubes.
pub struct NixieTubePair<A, B, C, D, E, F, G, H> {
    left: NixieTube<A, B, C, D>,
    right: NixieTube<E, F, G, H>,
}

impl<A, B, C, D, E, F, G, H> NixieTubePair<A, B, C, D, E, F, G, H>
where
    A: OutputPin,
    B: OutputPin,
    C: OutputPin,
    D: OutputPin,
    E: OutputPin,
    F: OutputPin,
    G: OutputPin,
    H: OutputPin,
{
    /// Create a new instance.
    pub fn new(left: NixieTube<A, B, C, D>, right: NixieTube<E, F, G, H>) -> Self {
        Self { left, right }
    }

    /// Return mutable reference to the left tube.
    pub fn left(&mut self) -> &mut NixieTube<A, B, C, D> {
        &mut self.left
    }

    /// Return mutable reference to the right tube.
    pub fn right(&mut self) -> &mut NixieTube<E, F, G, H> {
        &mut self.right
    }

    /// Show a number between 1 and 99.
    ///
    /// Leading zeroes as well as the number 0 will not be shown. If you need
    /// to show zeroes, use the `show_digit` method on the tube directly.
    pub fn show(&mut self, val: u8) {
        let tens = (val / 10) % 100;
        let ones = val % 10;
        if tens > 0 {
            self.left.show_digit(tens);
            self.right.show_digit(ones);
        } else if ones > 0 {
            self.left.off();
            self.right.show_digit(ones);
        } else {
            self.off();
        }
    }

    /// Turn off both tubes.
    pub fn off(&mut self) {
        self.left.off();
        self.right.off();
    }

    /// Show every digit on both tubes, with [`delay`] between each digit.
    pub async fn selftest(&mut self, delay: Duration) {
        for i in 0..=9 {
            self.left().show_digit(i);
            self.right().show_digit(i);
            Timer::after(delay).await;
        }
        self.off();
    }
}

impl<A, B, C, D> NixieTube<A, B, C, D>
where
    A: OutputPin,
    B: OutputPin,
    C: OutputPin,
    D: OutputPin,
{
    /// Show the specified digit.
    ///
    /// The value must be between 0 and 9. Otherwise, the tube will be turned off.
    pub fn show_digit(&mut self, digit: u8) {
        if digit & 0x01 > 0 {
            let _ = self.pin_a.set_high();
        } else {
            let _ = self.pin_a.set_low();
        }
        if digit & 0x02 > 0 {
            let _ = self.pin_b.set_high();
        } else {
            let _ = self.pin_b.set_low();
        }
        if digit & 0x04 > 0 {
            let _ = self.pin_c.set_high();
        } else {
            let _ = self.pin_c.set_low();
        }
        if digit & 0x08 > 0 {
            let _ = self.pin_d.set_high();
        } else {
            let _ = self.pin_d.set_low();
        }
    }

    /// Turn off the tube.
    pub fn off(&mut self) {
        // The value 0b1111 is out of range and will result
        // in the tube being turned off.
        let _ = self.pin_a.set_high();
        let _ = self.pin_b.set_high();
        let _ = self.pin_c.set_high();
        let _ = self.pin_d.set_high();
    }
}
