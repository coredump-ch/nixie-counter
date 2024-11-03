use std::time::Duration;

use esp_idf_svc::{
    hal::gpio::{AnyOutputPin, Output, PinDriver},
    timer::EspAsyncTimer,
};

/// A nixie tube.
///
/// The struct needs to be initialized with the four output pins connected to
/// the K155ID1 BCD encoder.
pub struct NixieTube<'a> {
    pub pin_a: PinDriver<'a, AnyOutputPin, Output>,
    pub pin_b: PinDriver<'a, AnyOutputPin, Output>,
    pub pin_c: PinDriver<'a, AnyOutputPin, Output>,
    pub pin_d: PinDriver<'a, AnyOutputPin, Output>,
}

/// A pair of two nixie tubes.
pub struct NixieTubePair<'a> {
    left: NixieTube<'a>,
    right: NixieTube<'a>,
}

impl<'a> NixieTubePair<'a> {
    /// Create a new instance.
    pub fn new(left: NixieTube<'a>, right: NixieTube<'a>) -> Self {
        Self { left, right }
    }

    /// Return mutable reference to the left tube.
    pub fn left(&mut self) -> &mut NixieTube<'a> {
        &mut self.left
    }

    /// Return mutable reference to the right tube.
    pub fn right(&mut self) -> &mut NixieTube<'a> {
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
    pub async fn selftest(&mut self, timer: &mut EspAsyncTimer, delay: Duration) -> () {
        for i in 0..=9 {
            self.left().show_digit(i);
            self.right().show_digit(i);
            let _ = timer.after(delay).await;
        }
        self.off();
    }
}

impl<'a> NixieTube<'a> {
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
