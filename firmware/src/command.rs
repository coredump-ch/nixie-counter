use at_rs::{utils, ATCommandInterface, ATRequestType, MaxCommandLen, MaxResponseLines};
use cortex_m_semihosting::hprintln;
use heapless::{ArrayLength, String, Vec};

#[derive(Debug, Clone)]
pub enum Command {
    /// AT attention command (`AT`), can be used to check whether everything is
    /// working as intended.
    At,
    /// Restart module (`AT+RST`).
    Restart,
    /// Get firmware version (`AT+GMR`).
    GetFirmwareVersion,
}

#[derive(Debug)]
pub enum Response {
    /// Response to `Command::At` and `Command::Restart`.
    Ready,
    /// Firmware version information.
    FirmwareVersion {
        at_version: String<MaxCommandLen>,
        sdk_version: String<MaxCommandLen>,
        compile_time: String<MaxCommandLen>,
    },
    /// Empty response.
    Empty,
    /// Unsolicited response.
    Unsolicited,
}

impl ATCommandInterface for Command {
    type Response = Response;

    fn get_cmd<N: ArrayLength<u8>>(&self) -> String<N> {
        match self {
            Command::At => String::from("AT"),
            Command::Restart => String::from("AT+RST"),
            Command::GetFirmwareVersion => String::from("AT+GMR"),
        }
    }

    fn parse_resp(
        &self,
        response_lines: &mut Vec<String<MaxCommandLen>, MaxResponseLines>,
    ) -> Response {
        // Handle empty response
        if response_lines.is_empty() {
            return Response::Empty;
        }

        // Split responses
        let mut responses: Vec<Vec<&str, MaxResponseLines>, MaxResponseLines> =
            utils::split_parameterized_resp(response_lines);

        // Get and handle response
        let response = responses.pop().unwrap();
        hprintln!("{:?}", response).unwrap();
        //match *self {
        //    Command::At => Response::Ready,
        //    Command::GetManufacturerId => Response::ManufacturerId {
        //        id: String::from(response[0]),
        //    },
        //    _ => Response::None,
        //}
        Response::Empty
    }

    fn parse_unsolicited(_response_line: &str) -> Option<Response> {
        Some(Response::Unsolicited)
    }
}


impl ATRequestType for Command {
    type Command = Command;

    fn try_get_cmd(self) -> Option<Self::Command> {
        Some(self)
    }

    fn get_bytes<N: ArrayLength<u8>>(&self) -> Vec<u8, N> {
        self.get_cmd().into_bytes()
    }
}

