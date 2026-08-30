#![no_main]

use gym_bot::mcp::decode_mcp_frame;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _request = decode_mcp_frame(data);
});
