use esp_idf_svc::{
    hal::gpio::{AnyIOPin, Input, PinDriver, Pull},
    sys::EspError,
};
use futures_util::{
    future::{select, try_join, Either},
    pin_mut,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

pub struct ToggleSwitch<'a, 'b> {
    pin_up: PinDriver<'a, AnyIOPin, Input>,
    pin_down: PinDriver<'b, AnyIOPin, Input>,
}

impl<'a, 'b> ToggleSwitch<'a, 'b> {
    /// Construct a new [`ToggleSwitch`] and enable internal pull-up resistors for both specified pins.
    pub fn new(pin_up: AnyIOPin, pin_down: AnyIOPin) -> anyhow::Result<Self> {
        let mut pin_up = PinDriver::input(pin_up)?;
        let mut pin_down = PinDriver::input(pin_down)?;
        pin_up.set_pull(Pull::Up)?;
        pin_down.set_pull(Pull::Up)?;
        Ok(Self { pin_up, pin_down })
    }

    /// Wait until the toggle switch is pressed or down
    pub async fn wait_for_press(&mut self) -> Direction {
        // Prepare futures
        let up_pressed = self.pin_up.wait_for_low();
        let down_pressed = self.pin_down.wait_for_low();

        // 'select' requires Future + Unpin bounds
        pin_mut!(up_pressed);
        pin_mut!(down_pressed);

        // Wait for up or down press
        match select(up_pressed, down_pressed).await {
            Either::Left(_) => Direction::Up,
            Either::Right(_) => Direction::Down,
        }
    }

    /// Wait until the toggle switch is pressed or down
    pub async fn wait_for_release(&mut self) -> anyhow::Result<(), EspError> {
        // Prepare futures
        let up_released = self.pin_up.wait_for_high();
        let down_released = self.pin_down.wait_for_high();

        // 'select' requires Future + Unpin bounds
        pin_mut!(up_released);
        pin_mut!(down_released);

        try_join(up_released, down_released).await?;
        Ok(())
    }
}
