//! Tag 32A — Value Date, Currency, Amount.
//!
//! Spec: `6n3a15d`
//!
//! - `6n` — 6 ASCII digits, `YYMMDD`. Calendar validity is checked
//!   (month 01..=12, day 01..=days_in_month including leap years).
//! - `3a` — 3 uppercase ASCII letters (ISO 4217 code; we only validate
//!   the *shape*, not membership in the ISO code table — currency lists
//!   change over time, and a dependency on a code table would dwarf
//!   the rest of this decoder).
//! - `15d` — up to 15 chars of SWIFT decimal: digits with exactly one
//!   `,` decimal separator. Per the spec the comma is mandatory even
//!   when the fractional part is empty (`1000,` is the canonical form
//!   for round amounts).

use super::{DecodeError, FieldSemantic, MtFieldDecoder};

const TAG: &str = "32A";
const DATE_LEN: usize = 6;
const CURRENCY_LEN: usize = 3;
const AMOUNT_MAX_LEN: usize = 15;

/// Zero-sized decoder for tag 32A.
#[derive(Debug, Clone, Copy, Default)]
pub struct Field32A;

impl MtFieldDecoder for Field32A {
    fn tag(&self) -> &'static str {
        TAG
    }

    fn decode(&self, raw: &str) -> Result<FieldSemantic, DecodeError> {
        if raw.len() < DATE_LEN + CURRENCY_LEN + 2 {
            // Minimum amount is "0," — at least 2 chars (one digit + comma).
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: raw.len(),
                max: DATE_LEN + CURRENCY_LEN + AMOUNT_MAX_LEN,
            });
        }
        let (date_part, rest) = raw.split_at(DATE_LEN);
        let (currency_part, amount_part) = rest.split_at(CURRENCY_LEN);
        validate_date(date_part)?;
        validate_currency(currency_part)?;
        validate_amount(amount_part)?;
        Ok(FieldSemantic::ValueDateAmount {
            date: date_part.to_string(),
            currency: currency_part.to_string(),
            amount: amount_part.to_string(),
        })
    }
}

fn validate_date(d: &str) -> Result<(), DecodeError> {
    if d.len() != DATE_LEN || !d.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DecodeError::InvalidDate {
            tag: TAG,
            value: d.to_string(),
        });
    }
    let yy: u32 = parse_pair(&d[0..2]);
    let mm: u32 = parse_pair(&d[2..4]);
    let dd: u32 = parse_pair(&d[4..6]);
    if !(1..=12).contains(&mm) {
        return Err(DecodeError::InvalidDate {
            tag: TAG,
            value: d.to_string(),
        });
    }
    // Year resolution: SWIFT treats YY as 20YY for the MT switchover
    // generation. Leap-year math just needs the full year, and the
    // arbitrary century pivot does not change results within the
    // 2000-2099 window the format actually targets.
    let full_year = 2000 + yy;
    let max_day = days_in_month(full_year, mm);
    if !(1..=max_day).contains(&dd) {
        return Err(DecodeError::InvalidDate {
            tag: TAG,
            value: d.to_string(),
        });
    }
    Ok(())
}

fn parse_pair(s: &str) -> u32 {
    // Safe: caller validated all bytes are ASCII digits with len == 2.
    let bytes = s.as_bytes();
    u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0')
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

fn is_leap_year(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn validate_currency(c: &str) -> Result<(), DecodeError> {
    if c.len() != CURRENCY_LEN || !c.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(DecodeError::InvalidCurrency {
            tag: TAG,
            value: c.to_string(),
        });
    }
    Ok(())
}

fn validate_amount(a: &str) -> Result<(), DecodeError> {
    if a.is_empty() || a.len() > AMOUNT_MAX_LEN {
        return Err(DecodeError::InvalidAmount {
            tag: TAG,
            value: a.to_string(),
        });
    }
    let mut seen_comma = false;
    let mut digit_count = 0usize;
    for b in a.bytes() {
        match b {
            b'0'..=b'9' => digit_count += 1,
            b',' => {
                if seen_comma {
                    return Err(DecodeError::InvalidAmount {
                        tag: TAG,
                        value: a.to_string(),
                    });
                }
                seen_comma = true;
            }
            _ => {
                return Err(DecodeError::InvalidAmount {
                    tag: TAG,
                    value: a.to_string(),
                });
            }
        }
    }
    // SWIFT MT requires the comma to be present (it is the decimal
    // separator, mandatory even for round amounts) and at least one
    // digit overall.
    if !seen_comma || digit_count == 0 {
        return Err(DecodeError::InvalidAmount {
            tag: TAG,
            value: a.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_value() {
        let out = Field32A.decode("240520USD1000,00").unwrap();
        match out {
            FieldSemantic::ValueDateAmount {
                date,
                currency,
                amount,
            } => {
                assert_eq!(date, "240520");
                assert_eq!(currency, "USD");
                assert_eq!(amount, "1000,00");
            }
            other => panic!("expected ValueDateAmount, got {other:?}"),
        }
    }

    #[test]
    fn accepts_leap_day() {
        // 2024 is divisible by 4 and not by 100 → leap year.
        let out = Field32A.decode("240229EUR1,00").unwrap();
        assert!(matches!(out, FieldSemantic::ValueDateAmount { .. }));
    }

    #[test]
    fn rejects_non_leap_feb_29() {
        // 2023 is not a leap year.
        let err = Field32A.decode("230229EUR1,00").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidDate { .. }));
    }

    #[test]
    fn rejects_month_zero() {
        let err = Field32A.decode("240020EUR1,00").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidDate { .. }));
    }

    #[test]
    fn rejects_lowercase_currency() {
        let err = Field32A.decode("240520usd1,00").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidCurrency { .. }));
    }

    #[test]
    fn rejects_amount_without_comma() {
        let err = Field32A.decode("240520USD1000").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAmount { .. }));
    }

    #[test]
    fn rejects_amount_with_two_commas() {
        let err = Field32A.decode("240520USD1,000,00").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidAmount { .. }));
    }

    #[test]
    fn rejects_too_short_total() {
        let err = Field32A.decode("240520USD").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidLength { .. }));
    }
}
