//! Small, bounded HTTP request router, also testable on the host.
#[derive(Debug, PartialEq)]
pub enum Route {
    Ota,
    Page,
    Logo,
    Gpio,
    SetRelay(bool),
    AddAuthorized,
    DeleteAuthorized,
    SetRelayDuration,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    BadRequest,
}

pub fn headers_complete(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|window| window == b"\r\n\r\n")
}

pub fn route(bytes: &[u8]) -> Route {
    let end = header_end(bytes).unwrap_or(bytes.len());
    let Ok(text) = core::str::from_utf8(&bytes[..end]) else {
        return Route::BadRequest;
    };
    let Some(line) = text.split("\r\n").next() else {
        return Route::BadRequest;
    };
    let mut words = line.split_whitespace();
    let (Some(method), Some(path), Some(version)) = (words.next(), words.next(), words.next())
    else {
        return Route::BadRequest;
    };
    if words.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Route::BadRequest;
    }
    let path = path.split('?').next().unwrap_or("");
    if path == "/api/ota" {
        if method != "POST" {
            return Route::MethodNotAllowed;
        }
        return if text
            .split("\r\n")
            .skip(1)
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| name.eq_ignore_ascii_case("x-ota-control") && value.trim() == "1")
        {
            Route::Ota
        } else {
            Route::Forbidden
        };
    }
    if matches!(path, "/api/relay/high" | "/api/relay/low") {
        if method != "POST" {
            return Route::MethodNotAllowed;
        }
        // Custom header prevents ordinary cross-origin forms from operating the
        // relay. Cross-origin scripts require a CORS preflight, which we reject.
        let authorized = text
            .split("\r\n")
            .skip(1)
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| {
                name.eq_ignore_ascii_case("x-relay-control") && value.trim() == "1"
            });
        if !authorized {
            return Route::Forbidden;
        }
        return Route::SetRelay(path == "/api/relay/high");
    }
    if matches!(path, "/api/authorized/add" | "/api/authorized/delete") {
        if method != "POST" {
            return Route::MethodNotAllowed;
        }
        let authorized = text
            .split("\r\n")
            .skip(1)
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| {
                name.eq_ignore_ascii_case("x-authorized-control") && value.trim() == "1"
            });
        if !authorized {
            return Route::Forbidden;
        }
        return if path == "/api/authorized/add" {
            Route::AddAuthorized
        } else {
            Route::DeleteAuthorized
        };
    }
    if path == "/api/relay-duration" {
        if method != "POST" {
            return Route::MethodNotAllowed;
        }
        let authorized = text
            .split("\r\n")
            .skip(1)
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.split_once(':'))
            .any(|(name, value)| {
                name.eq_ignore_ascii_case("x-relay-control") && value.trim() == "1"
            });
        return if authorized {
            Route::SetRelayDuration
        } else {
            Route::Forbidden
        };
    }
    if method != "GET" {
        return Route::MethodNotAllowed;
    }
    match path {
        "/" => Route::Page,
        "/nekodoa.png" => Route::Logo,
        "/api/gpio" => Route::Gpio,
        _ => Route::NotFound,
    }
}

/// Extract and validate the decimal ID in `?id=...` without allocating.
pub fn authorized_id(bytes: &[u8]) -> Option<&[u8]> {
    let text = core::str::from_utf8(bytes).ok()?;
    let path = text.split_whitespace().nth(1)?;
    let value = path.split_once('?')?.1.strip_prefix("id=")?;
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(value.as_bytes())
}

pub fn relay_seconds(bytes: &[u8]) -> Option<u16> {
    let text = core::str::from_utf8(bytes).ok()?;
    let path = text.split_whitespace().nth(1)?;
    let value = path.split_once('?')?.1.strip_prefix("seconds=")?;
    let seconds = value.parse().ok()?;
    (1..=300).contains(&seconds).then_some(seconds)
}

/// Offset of body; binary bytes received with headers must be retained.
pub fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
}
pub fn ota_length(bytes: &[u8]) -> Option<usize> {
    let text = core::str::from_utf8(&bytes[..header_end(bytes)?]).ok()?;
    let mut length = None;
    for line in text.split("\r\n").skip(1).take_while(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return None;
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            length = Some(value.parse::<usize>().ok()?);
        }
    }
    length.filter(|&n| (288..=0x1e0000).contains(&n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_rejects_invalid_requests() {
        assert_eq!(route(b"GET / HTTP/1.1\r\n\r\n"), Route::Page);
        assert_eq!(route(b"GET /api/gpio?t=1 HTTP/1.0\r\n\r\n"), Route::Gpio);
        assert_eq!(route(b"GET /nekodoa.png HTTP/1.1\r\n\r\n"), Route::Logo);
        assert_eq!(
            route(b"POST /api/authorized/add?id=1 HTTP/1.1\r\nX-Authorized-Control: 1\r\n\r\n"),
            Route::AddAuthorized
        );
        assert_eq!(
            route(b"POST /api/relay-duration?seconds=10 HTTP/1.1\r\nX-Relay-Control: 1\r\n\r\n"),
            Route::SetRelayDuration
        );
        assert_eq!(route(b"GET /missing HTTP/1.1\r\n\r\n"), Route::NotFound);
        assert_eq!(
            route(b"POST /api/gpio HTTP/1.1\r\n\r\n"),
            Route::MethodNotAllowed
        );
        for bytes in [
            b"GET /".as_slice(),
            b"GET / HTTP/2",
            b"GET / HTTP/1.1 extra",
            b"\xff",
        ] {
            assert_eq!(route(bytes), Route::BadRequest);
        }
    }

    #[test]
    fn accepts_reported_tag_id_without_numeric_truncation() {
        let request = b"POST /api/authorized/add?id=900255004170212 HTTP/1.1\r\nX-Authorized-Control: 1\r\n\r\n";
        assert_eq!(route(request), Route::AddAuthorized);
        assert_eq!(authorized_id(request), Some(b"900255004170212".as_slice()));
    }

    #[test]
    fn waits_for_fragmented_headers() {
        let request = b"GET /api/gpio HTTP/1.1\r\nHost: esp32\r\n\r\n";
        for length in 0..request.len() {
            assert!(!headers_complete(&request[..length]));
        }
        assert!(headers_complete(request));
    }

    #[test]
    fn relay_requires_explicit_post_and_control_header() {
        assert_eq!(
            route(b"POST /api/relay/high HTTP/1.1\r\nX-Relay-Control: 1\r\n\r\n"),
            Route::SetRelay(true)
        );
        assert_eq!(
            route(b"POST /api/relay/low HTTP/1.1\r\nx-relay-control: 1\r\n\r\n"),
            Route::SetRelay(false)
        );
        assert_eq!(
            route(b"GET /api/relay/low HTTP/1.1\r\nX-Relay-Control: 1\r\n\r\n"),
            Route::MethodNotAllowed
        );
        assert_eq!(
            route(b"POST /api/relay/low HTTP/1.1\r\n\r\n"),
            Route::Forbidden
        );
        assert_eq!(
            route(b"POST /api/relay/low HTTP/1.1\r\n\r\nX-Relay-Control: 1"),
            Route::Forbidden
        );
        assert_eq!(
            route(b"OPTIONS /api/relay/low HTTP/1.1\r\n\r\n"),
            Route::MethodNotAllowed
        );
        assert_eq!(
            route(b"POST /api/gpio/25/low HTTP/1.1\r\nX-Relay-Control: 1\r\n\r\n"),
            Route::MethodNotAllowed
        );
    }
}
