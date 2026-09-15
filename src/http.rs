//! Small, bounded HTTP request router, also testable on the host.
#[derive(Debug, PartialEq)]
pub enum Route {
    Page,
    Gpio,
    SetRelay(bool),
    Forbidden,
    NotFound,
    MethodNotAllowed,
    BadRequest,
}

pub fn headers_complete(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|window| window == b"\r\n\r\n")
}

pub fn route(bytes: &[u8]) -> Route {
    let Ok(text) = core::str::from_utf8(bytes) else {
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
    if method != "GET" {
        return Route::MethodNotAllowed;
    }
    match path {
        "/" => Route::Page,
        "/api/gpio" => Route::Gpio,
        _ => Route::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_rejects_invalid_requests() {
        assert_eq!(route(b"GET / HTTP/1.1\r\n\r\n"), Route::Page);
        assert_eq!(route(b"GET /api/gpio?t=1 HTTP/1.0\r\n\r\n"), Route::Gpio);
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
