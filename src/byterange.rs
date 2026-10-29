//! HTTP `Range` request handling for file downloads (RFC 9110 section 14.2).
//!
//! Only a single `bytes=` range is honoured. Anything this doesn't understand,
//! including a multi-range request, is answered with the whole file, which the
//! RFC explicitly allows ("a server MAY ignore the Range header field").

/// How to answer a request for a file of a known length.
#[derive(Debug, PartialEq, Eq)]
pub enum Range {
    /// No usable `Range` header: send everything with a plain 200.
    Full,
    /// Send bytes `start..=end` (inclusive, both inside the file) as a 206.
    Partial { start: u64, end: u64 },
    /// The range starts past the end (or is empty): answer 416.
    Unsatisfiable,
}

/// Interpret a `Range` header value against a file of `len` bytes.
pub fn parse(header: Option<&str>, len: u64) -> Range {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Range::Full; // absent, or a unit we don't know
    };
    if spec.contains(',') {
        return Range::Full; // multi-range: not supported, so serve it all
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return Range::Full;
    };
    let (first, last) = (first.trim(), last.trim());

    match (first.is_empty(), last.is_empty()) {
        (true, true) => Range::Full,
        // "-N": the last N bytes.
        (true, false) => match last.parse::<u64>() {
            Ok(0) => Range::Unsatisfiable,
            Ok(_) if len == 0 => Range::Unsatisfiable,
            Ok(n) => Range::Partial {
                start: len.saturating_sub(n),
                end: len - 1,
            },
            Err(_) => Range::Full,
        },
        // "A-" or "A-B".
        (false, _) => {
            let Ok(start) = first.parse::<u64>() else {
                return Range::Full;
            };
            let end = if last.is_empty() {
                None
            } else {
                match last.parse::<u64>() {
                    Ok(e) if e < start => return Range::Full, // malformed: ignore it
                    Ok(e) => Some(e),
                    Err(_) => return Range::Full,
                }
            };
            if start >= len {
                Range::Unsatisfiable
            } else {
                Range::Partial {
                    start,
                    end: end.map_or(len - 1, |e| e.min(len - 1)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(h: &str, len: u64) -> Range {
        parse(Some(h), len)
    }
    fn part(start: u64, end: u64) -> Range {
        Range::Partial { start, end }
    }

    #[test]
    fn no_header_or_a_foreign_unit_means_the_whole_file() {
        assert_eq!(parse(None, 100), Range::Full);
        assert_eq!(p("items=0-5", 100), Range::Full);
        assert_eq!(p("", 100), Range::Full);
    }

    #[test]
    fn explicit_open_ended_and_suffix_ranges() {
        assert_eq!(p("bytes=0-9", 100), part(0, 9));
        assert_eq!(p("bytes=10-", 100), part(10, 99));
        assert_eq!(p("bytes=-10", 100), part(90, 99));
        assert_eq!(p("bytes=0-0", 100), part(0, 0));
        assert_eq!(p("  bytes=5-7  ", 100), part(5, 7));
    }

    #[test]
    fn an_end_past_the_file_is_clamped_and_an_oversize_suffix_is_the_whole_file() {
        assert_eq!(p("bytes=90-9999", 100), part(90, 99));
        assert_eq!(p("bytes=-9999", 100), part(0, 99));
    }

    #[test]
    fn a_start_at_or_past_the_end_is_unsatisfiable() {
        assert_eq!(p("bytes=100-", 100), Range::Unsatisfiable);
        assert_eq!(p("bytes=9999-99999", 100), Range::Unsatisfiable);
        assert_eq!(p("bytes=-0", 100), Range::Unsatisfiable);
        // Nothing in an empty file can be selected.
        assert_eq!(p("bytes=0-", 0), Range::Unsatisfiable);
        assert_eq!(p("bytes=-5", 0), Range::Unsatisfiable);
    }

    #[test]
    fn malformed_or_unsupported_forms_are_ignored_not_errors() {
        for h in [
            "bytes=",
            "bytes=-",
            "bytes=abc",
            "bytes=a-b",
            "bytes=5-2",
            "bytes=1-2-3",
            "bytes=0-1,5-9",
        ] {
            assert_eq!(p(h, 100), Range::Full, "{h}");
        }
    }

    #[test]
    fn huge_numbers_dont_overflow() {
        assert_eq!(p("bytes=0-18446744073709551615", 100), part(0, 99));
        assert_eq!(p("bytes=18446744073709551615-", 100), Range::Unsatisfiable);
        assert_eq!(p("bytes=-18446744073709551615", 100), part(0, 99));
        assert_eq!(p("bytes=99999999999999999999999-", 100), Range::Full); // doesn't fit u64
    }
}
