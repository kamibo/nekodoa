use crate::http::{self, Route};
use alloc::format;
use embassy_net::{
    Stack,
    tcp::{Error, TcpSocket},
};
use embassy_time::{Duration, with_timeout};

const PAGE: &str = include_str!("index.html");
const LOGO: &[u8] = include_bytes!("nekodoa.png");

#[embassy_executor::task(pool_size = 2)]
pub async fn serve(stack: Stack<'static>) {
    let mut rx = [0; 2048];
    let mut tx = [0; 2048];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(10)));
    loop {
        stack.wait_config_up().await;
        crate::ota::http_ready();
        if socket.accept(80).await.is_ok() {
            // Header/upload reads and response writes each enforce absolute deadlines.
            // Once metadata is committed, no outer timeout may cancel the restart.
            if handle(&mut socket).await.is_ok() {
                socket.close();
                let _ = with_timeout(Duration::from_secs(2), socket.flush()).await;
            }
        }
        socket.abort();
        let _ = with_timeout(Duration::from_secs(1), socket.flush()).await;
    }
}

async fn handle(socket: &mut TcpSocket<'_>) -> Result<(), Error> {
    let mut request = [0; 1024];
    let mut used = 0;
    let header_start = embassy_time::Instant::now();
    while !http::headers_complete(&request[..used]) {
        if used == request.len() {
            return respond(
                socket,
                "431 Request Header Fields Too Large",
                "text/plain",
                "Headers too large",
            )
            .await;
        }
        let count = match with_timeout(
            Duration::from_millis(5000u64.saturating_sub(header_start.elapsed().as_millis())),
            socket.read(&mut request[used..]),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => return Ok(()),
        };
        if count == 0 {
            return Ok(());
        }
        used += count;
    }
    let end = http::header_end(&request[..used]).unwrap();
    let route = http::route(&request[..end]);
    if crate::relay::updating()
        && matches!(
            route,
            Route::SetRelay(_)
                | Route::AddAuthorized
                | Route::DeleteAuthorized
                | Route::SetRelayDuration
        )
    {
        return respond(
            socket,
            "409 Conflict",
            "text/plain",
            "Firmware update in progress",
        )
        .await;
    }
    match route {
        Route::Ota => upload(socket, &request[..end], &request[end..used]).await,
        Route::SetRelay(high) => {
            if !crate::relay::set(high) {
                return respond(
                    socket,
                    "409 Conflict",
                    "text/plain",
                    "Firmware update in progress",
                )
                .await;
            }
            let json = crate::gpio::snapshot();
            respond(socket, "200 OK", "application/json", &json).await
        }
        Route::AddAuthorized | Route::DeleteAuthorized => {
            let Some(id) = http::authorized_id(&request[..used]) else {
                return respond(
                    socket,
                    "400 Bad Request",
                    "text/plain",
                    "Use ?id=decimal-id",
                )
                .await;
            };
            let changed = if matches!(http::route(&request[..used]), Route::AddAuthorized) {
                crate::rfid::add_authorized(id)
            } else {
                crate::rfid::delete_authorized(id)
            };
            if !changed {
                return respond(
                    socket,
                    "409 Conflict",
                    "text/plain",
                    "Authorization list is full or ID was not present",
                )
                .await;
            }
            let json = crate::gpio::snapshot();
            respond(socket, "200 OK", "application/json", &json).await
        }
        Route::SetRelayDuration => {
            let Some(seconds) = http::relay_seconds(&request[..used]) else {
                return respond(
                    socket,
                    "400 Bad Request",
                    "text/plain",
                    "Use ?seconds=1..300",
                )
                .await;
            };
            if !crate::rfid::set_relay_seconds(seconds) {
                return respond(
                    socket,
                    "400 Bad Request",
                    "text/plain",
                    "Duration must be 1..300 seconds",
                )
                .await;
            }
            let json = crate::gpio::snapshot();
            respond(socket, "200 OK", "application/json", &json).await
        }
        Route::Forbidden => {
            respond(
                socket,
                "403 Forbidden",
                "text/plain",
                "Missing relay control header",
            )
            .await
        }
        Route::Page => respond(socket, "200 OK", "text/html; charset=utf-8", PAGE).await,
        Route::Logo => respond_bytes(socket, "200 OK", "image/png", LOGO).await,
        Route::Gpio => {
            let json = crate::gpio::snapshot();
            respond(socket, "200 OK", "application/json", &json).await
        }
        Route::NotFound => respond(socket, "404 Not Found", "text/plain", "Not found").await,
        Route::MethodNotAllowed => {
            respond(
                socket,
                "405 Method Not Allowed",
                "text/plain",
                "Use GET for monitoring or POST for relay control",
            )
            .await
        }
        Route::BadRequest => respond(socket, "400 Bad Request", "text/plain", "Bad request").await,
    }
}

async fn respond(
    socket: &mut TcpSocket<'_>,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), Error> {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nAllow: GET, POST\r\n\r\n",
        body.len()
    );
    write_all(socket, headers.as_bytes()).await?;
    write_all(socket, body.as_bytes()).await?;
    with_timeout(Duration::from_secs(2), socket.flush())
        .await
        .unwrap_or(Err(Error::ConnectionReset))
}

async fn respond_bytes(
    socket: &mut TcpSocket<'_>,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), Error> {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: public, max-age=86400\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    );
    write_all(socket, headers.as_bytes()).await?;
    write_all(socket, body).await?;
    with_timeout(Duration::from_secs(2), socket.flush())
        .await
        .unwrap_or(Err(Error::ConnectionReset))
}

async fn write_all(socket: &mut TcpSocket<'_>, mut bytes: &[u8]) -> Result<(), Error> {
    let started = embassy_time::Instant::now();
    while !bytes.is_empty() {
        let count = with_timeout(
            Duration::from_millis(5000u64.saturating_sub(started.elapsed().as_millis())),
            socket.write(bytes),
        )
        .await
        .map_err(|_| Error::ConnectionReset)??;
        if count == 0 {
            return Err(Error::ConnectionReset);
        }
        bytes = &bytes[count..];
    }
    Ok(())
}

async fn upload(socket: &mut TcpSocket<'_>, headers: &[u8], initial: &[u8]) -> Result<(), Error> {
    let Some(length) = http::ota_length(headers) else {
        return respond(
            socket,
            "400 Bad Request",
            "text/plain",
            "Require one Content-Length (288..1966080); no Transfer-Encoding",
        )
        .await;
    };
    if initial.len() > length {
        return respond(
            socket,
            "400 Bad Request",
            "text/plain",
            "Body exceeds Content-Length",
        )
        .await;
    }
    let mut update = match crate::ota::Upload::begin(length) {
        Ok(update) => update,
        Err(message) => return respond(socket, "409 Conflict", "text/plain", message).await,
    };
    let mut buffer = crate::ota::Buffer([0xff; 1024]);
    let mut received = 0;
    let mut filled = initial.len();
    buffer.0[..filled].copy_from_slice(initial);
    let upload_start = embassy_time::Instant::now();
    let mut last_activity = embassy_time::Instant::now();
    while received < length {
        if upload_start.elapsed().as_secs() >= 300 {
            return respond(
                socket,
                "408 Request Timeout",
                "text/plain",
                "Upload exceeded five minutes",
            )
            .await;
        }
        let target = core::cmp::min(1024, length - received);
        while filled < target {
            let timeout = Duration::from_millis(core::cmp::min(
                10_000u64.saturating_sub(last_activity.elapsed().as_millis()),
                300_000u64.saturating_sub(upload_start.elapsed().as_millis()),
            ));
            let count =
                match with_timeout(timeout, socket.read(&mut buffer.0[filled..target])).await {
                    Ok(Ok(count)) if count > 0 => count,
                    _ => {
                        return respond(
                            socket,
                            "408 Request Timeout",
                            "text/plain",
                            "Upload interrupted or idle for 10 seconds",
                        )
                        .await;
                    }
                };
            last_activity = embassy_time::Instant::now();
            filled += count;
        }
        if let Err(message) = update.chunk(&mut buffer, target).await {
            return respond(socket, "400 Bad Request", "text/plain", message).await;
        }
        received += target;
        filled = 0;
    }
    if let Err(message) = update.activate(upload_start) {
        return respond(socket, "400 Bad Request", "text/plain", message).await;
    }
    // Activation is committed; even a lost response must lead to a reset.
    let _ = with_timeout(
        Duration::from_secs(2),
        respond(
            socket,
            "200 OK",
            "text/plain",
            "Firmware accepted. Restarting; reconnect after startup verification.",
        ),
    )
    .await;
    socket.close();
    let _ = with_timeout(Duration::from_secs(2), socket.flush()).await;
    embassy_time::Timer::after_millis(250).await;
    esp_hal::system::software_reset();
}
