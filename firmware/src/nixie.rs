use embedded_hal::digital::v2::OutputPin;

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

    /// Turn off both tubes.
    pub fn off(&mut self) {
        self.left.off();
        self.right.off();
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
    pub fn show(&mut self, digit: u8) {
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
