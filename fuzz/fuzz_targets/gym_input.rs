#![no_main]

use gym_bot::{command::parse_weight_command, telegram::decode_update_json};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let _weight = parse_weight_command(source);
        let _telegram = decode_update_json(source);
    }
});
