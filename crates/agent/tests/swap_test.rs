#![cfg(test)]

use multitop_agent::proc::Usage;
use multitop_agent::proto::{decode_packet, encode_packet, Payload, HEADER_LEN};
use multitop_agent::render::Snapshot;

#[test]
fn linux_swap_counts_capacity_and_used_bytes_including_disabled_swap() {
    let swap = multitop_agent::swap::parse_meminfo("SwapTotal: 8388608 kB\nSwapFree: 6291456 kB\n")
        .unwrap();
    assert_eq!(swap.total, 8 << 30);
    assert_eq!(swap.used, 2 << 30);
    assert_eq!(
        multitop_agent::swap::parse_meminfo("SwapTotal: 0 kB\nSwapFree: 0 kB\n"),
        Some(Usage::default())
    );
    assert!(multitop_agent::swap::parse_meminfo("MemTotal: 123 kB\n").is_none());
}

#[test]
fn swap_survives_the_wire_without_confusing_missing_and_zero() {
    for swap in [
        None,
        Some(Usage::default()),
        Some(Usage::new(8 << 30, 2 << 30)),
    ] {
        let bytes = encode_packet(&Payload::Monitor(Snapshot {
            swap,
            ..Snapshot::default()
        }));
        let Some(Payload::Monitor(got)) = decode_packet(&bytes) else {
            panic!("invalid snapshot")
        };
        assert_eq!(got.swap, swap);
    }
}

#[test]
fn an_old_collector_without_swap_still_decodes_with_an_unavailable_reading() {
    let mut bytes = encode_packet(&Payload::Monitor(Snapshot::default()));
    bytes.pop(); // Version 5 has no trailing swap-present flag.
    bytes[4] = 5;
    let len = u16::try_from(bytes.len() - HEADER_LEN).unwrap();
    bytes[6..8].copy_from_slice(&len.to_le_bytes());
    let Some(Payload::Monitor(got)) = decode_packet(&bytes) else {
        panic!("old collector rejected")
    };
    assert!(got.swap.is_none());
}
