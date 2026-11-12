//! Swap allocation and usage. An unavailable sample is distinct from zero swap.

use crate::proc::Usage;

/// Read Linux's configured swap capacity and usage from `/proc/meminfo`.
#[must_use]
pub fn parse_meminfo(text: &str) -> Option<Usage> {
    let mut total = None;
    let mut free = None;
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let slot = match key {
            "SwapTotal" => &mut total,
            "SwapFree" => &mut free,
            _ => continue,
        };
        *slot = rest
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            .checked_mul(1024);
    }
    let total = total?;
    Some(Usage::new(total, total.saturating_sub(free?)))
}

#[must_use]
pub fn sample() -> Option<Usage> {
    std::fs::read_to_string("/proc/meminfo").map_or_else(
        |_| crate::sys::get_swap_macos(),
        |text| parse_meminfo(&text),
    )
}
