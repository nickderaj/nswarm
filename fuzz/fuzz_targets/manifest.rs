#![no_main]

use fleet::BotManifest;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let _result = BotManifest::parse(source);
    }
});
