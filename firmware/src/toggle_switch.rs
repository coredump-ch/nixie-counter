use embassy_futures::{
    join::join,
    select::{select, Either},
};
use esp_hal::{
    gpio::{Input, InputPin},
    peripheral::Peripheral,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

pub struct ToggleSwitch<'a, 'b> {
    pin_up: Input<'a>,
    pin_down: Input<'b>,
}

impl<'a, 'b> ToggleSwitch<'a, 'b> {
    /// Construct a new [`ToggleSwitch`] and enable internal pull-up resistors for both specified pins.
    pub fn new(
        pin_up: impl Peripheral<P = impl InputPin> + 'a,
        pin_down: impl Peripheral<P = impl InputPin> + 'b,
    ) -> Self {
        Self {
            pin_up: Input::new(pin_up, esp_hal::gpio::Pull::Up),
            pin_down: Input::new(pin_down, esp_hal::gpio::Pull::Up),
        }
    }

    /// Wait until the toggle switch is pressed or down
    pub async fn wait_for_press(&mut self) -> Direction {
        // Prepare futures
        let up_pressed = self.pin_up.wait_for_low();
        let down_pressed = self.pin_down.wait_for_low();

        // Wait for up or down press
        match select(up_pressed, down_pressed).await {
            Either::First(_) => Direction::Up,
            Either::Second(_) => Direction::Down,
        }
    }

    /// Wait until the toggle switch is pressed or down
    pub async fn wait_for_release(&mut self) {
        let up_released = self.pin_up.wait_for_high();
        let down_released = self.pin_down.wait_for_high();
        join(up_released, down_released).await;
    }
}
