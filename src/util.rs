use bigdecimal::num_traits::Pow;
use once_cell::sync::OnceCell;
use sysinfo::{System, SystemExt};

const MICROSECOND: u128 = 1000;
const MILLISECOND: u128 = MICROSECOND * 1000;
const SECOND: u128 = MILLISECOND * 1000;
const MINUTE: u128 = SECOND * 60;
const HOUR: u128 = MINUTE * 6;
const DAY: u128 = HOUR * 24;

pub fn format_duration(time_nanos: u128) -> String {
    match time_nanos {
        t if t < MICROSECOND => format!("{t}ns"),
        t if t < MILLISECOND => format_unit(t, "us", MICROSECOND, 3),
        t if t < SECOND => format_unit(t, "ms", MILLISECOND, 3),
        t if t < MINUTE => format_unit(t, "s", SECOND, 3),
        t if t < HOUR => format_unit(t, "m", MINUTE, 3),
        t if t < DAY => format_unit(t, "h", HOUR, 3),
        t => format_unit(t, "d", DAY, 3),
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
fn format_unit(
    nanos: u128,
    units_label: &str,
    units_factor: u128,
    num_fractional_digits: i32,
) -> String {
    let integer_digits = nanos / units_factor;
    let fraction_digits = (((nanos % units_factor) as f64 / units_factor as f64)
        * 10.0.pow(num_fractional_digits)) as u64;
    format!("{integer_digits}.{fraction_digits}{units_label}")
}

#[allow(clippy::cast_precision_loss)]
pub fn format_duration_f64(time_nanos: f64) -> String {
    match time_nanos {
        t if t < MICROSECOND as f64 => format!("{t}ns"),
        t if t < MILLISECOND as f64 => format!("{:.3}us", t / (MICROSECOND as f64)),
        t if t < SECOND as f64 => format!("{:.3}ms", t / (MILLISECOND as f64)),
        t if t < MINUTE as f64 => format!("{:.3}s", t / (SECOND as f64)),
        t if t < HOUR as f64 => format!("{:.3}m", t / (MINUTE as f64)),
        t if t < DAY as f64 => format!("{:.3}h", t / (HOUR as f64)),
        t => format!("{:.3}d", t / (DAY as f64)),
    }
}

static USER_AGENT: OnceCell<String> = OnceCell::new();
pub fn user_agent<'a>() -> &'a str {
    &USER_AGENT.get_or_init(|| {
        let mut sys = System::new_all();
        sys.refresh_all();

        format!(
            "loaded/{} {:?}/{:?}",
            env!("CARGO_PKG_VERSION"),
            sys.name(),
            sys.kernel_version()
        )
    })
}

// Divvys up the `to_divvy` value across `num_items` yielding an iterator of equivalent len
pub fn divvy(to_divvy: usize, num_items: usize) -> impl Iterator<Item = usize> {
    let num_per_item = to_divvy / num_items;
    let num_per_item_remainder = to_divvy % num_items;

    (0..num_items).map(move |i| {
        if i < num_per_item_remainder {
            num_per_item + 1
        } else {
            num_per_item
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter;

    #[test]
    fn test_divvy_no_remainder() {
        let actual = divvy(25, 5);
        let expected = iter::repeat(5).take(5);
        assert!(actual.eq(expected));
    }

    #[test]
    fn test_divvy_with_remainder() {
        let actual = divvy(29, 5);
        let expected = [6, 6, 6, 6, 5].into_iter();
        assert!(actual.eq(expected));
    }
}
