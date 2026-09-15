use crate::http::{self, Route};
use alloc::format;
use embassy_net::{
    Stack,
    tcp::{Error, TcpSocket},
};
use embassy_time::{Duration, with_timeout};

const PAGE: &str = include_str!("index.html");

#[embassy_executor::task(pool_size = 2)]
pub async fn serve(stack: Stack<'static>) {
    let mut rx = [0; 2048];
    let mut tx = [0; 2048];
    let mut socket = TcpSocket::new(stack, &mut rx, &mut tx);
    socket.set_timeout(Some(Duration::from_secs(5)));
    loop {
        stack.wait_config_up().await;
        if socket.accept(80).await.is_ok() {
            // Bound the entire request, including clients trickling partial headers.
            if matches!(
                with_timeout(Duration::from_secs(5), handle(&mut socket)).await,
                Ok(Ok(()))
            ) {
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
        let count = socket.read(&mut request[used..]).await?;
        if count == 0 {
            return Ok(());
        }
        used += count;
    }
    match http::route(&request[..used]) {
        Route::SetRelay(high) => {
            crate::relay::set(high);
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
    socket.flush().await
}

async fn write_all(socket: &mut TcpSocket<'_>, mut bytes: &[u8]) -> Result<(), Error> {
    while !bytes.is_empty() {
        let count = socket.write(bytes).await?;
        if count == 0 {
            return Err(Error::ConnectionReset);
        }
        bytes = &bytes[count..];
    }
    Ok(())
}
