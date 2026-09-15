# nekodoa

Minimal `no_std` Rust firmware using Embassy on an ELEGOO ESP-WROOM-32
development board (original ESP32 / Xtensa). If your board is an ESP32-C3,
C6, or S3, it needs a different target and chip features.

The firmware logs startup and connection changes over UART0, accessible through
the board's USB serial bridge at 115200 baud.

Wi-Fi connects to `las capelles 12`, obtains an IPv4 address using DHCP, and
logs connection changes and the assigned address. It retries five seconds
after a failed connection or disconnect.

## Setup

Install [Rust](https://rustup.rs/), then install the
[Espressif toolchain](https://github.com/esp-rs/espup) and serial flasher:

```sh
cargo +stable install espup --locked
espup install --targets esp32
cargo +stable install espflash --locked
```

On Linux/macOS, load the toolchain environment in each new terminal:

```sh
source "$HOME/export-esp.sh"
```

`rust-toolchain.toml` selects the installed `esp` toolchain automatically.

## Build and run

Set the Wi-Fi password before building. Copy `wifi.example.toml` to `wifi.toml`
(ignored by Git), then edit its `WIFI_PASSWORD` value. An empty string selects
an open network; a nonempty password selects WPA2-Personal. The network must
be available on 2.4 GHz for this ESP32 board.

Build using the local configuration:

```sh
cargo --config wifi.toml build --release --locked
```

Alternatively, provide `WIFI_PASSWORD` as an environment variable when running
Cargo. The password is embedded in the firmware but never logged. Changing it
requires rebuilding and flashing. With no `WIFI_PASSWORD` set, the firmware
logs a setup warning and leaves the relay output HIGH with networking disabled.

To build without Wi-Fi credentials:

```sh
cargo build --release --locked
```

Connect the board with a USB data cable, then flash and open the serial monitor:

```sh
cargo --config wifi.toml run --release --locked
```

To select a serial port explicitly:

```sh
cargo --config wifi.toml run --release --locked -- --port /dev/ttyUSB0
```

Replace the port with your board's device (for example `COM3` on Windows).
If the flasher cannot connect, hold BOOT while it connects and release it
once flashing starts. On Linux, ensure your user has access to the serial port.
Exit the monitor with Ctrl+C.

If serial output is unreadable, close other serial monitors and reopen it with
the firmware's explicit settings (115200 baud, ordinary serial text):

```sh
espflash monitor --chip esp32 --monitor-baud 115200 --log-format serial
```

Add `--port /dev/ttyUSB0` if needed. Other terminal programs should use 115200
baud, 8 data bits, no parity, 1 stop bit, and no flow control. `--baud` controls
the flashing connection; `--monitor-baud` controls the console. The Cargo runner
sets the console speed and format explicitly. If output is still unreadable,
note whether the reset/boot messages are readable before the application starts.

The implementation follows the official
[esp-rs Embassy template](https://github.com/esp-rs/esp-generate/blob/main/template/src/bin/async_main.rs):
`esp-hal` initializes the chip, `esp-rtos` integrates the Embassy executor and
timer driver. Network status logging waits for changes asynchronously.
`ESP_LOG` defaults to `info` in `.cargo/config.toml`; changes to that setting
take effect when rebuilding the firmware.

Wi-Fi initialization and reconnection follow Espressif's
[esp-radio 0.18 DHCP example](https://github.com/esp-rs/esp-hal/blob/esp-radio-v0.18.0/examples/wifi/embassy_dhcp/src/main.rs).

## GPIO web page

After flashing, open the `GPIO dashboard: http://…/` address printed in the
serial monitor from a device on the same Wi-Fi network. The ESP32 serves the
page on port 80, with no internet connection or external assets required.
The page polls `GET /api/gpio` once per second and marks old readings as stale
when the board is unreachable. Wi-Fi reconnects automatically.

All GPIO numbers 0–39 are listed with a digital level, configuration and usage
notes. Missing, unexposed and flash pins are marked unavailable/reserved.
The monitor only reads registers: it does not enable inputs, change pulls,
or drive pins. Disabled digital inputs and RTC/analog pins show unavailable;
software outputs may show a separately labelled output latch value.
Floating inputs can change randomly, and these samples do not measure voltage
or capture fast pulses. The page also provides a dedicated control for GPIO26;
all other GPIOs remain read-only. Anyone able to reach the ESP32 on your network
can use the relay control; the page has no login.

Register interpretation follows the
[ESP32 technical reference manual](https://www.espressif.com/sites/default/files/documentation/esp32_technical_reference_manual_en.pdf).

## Door relay

GPIO26 is assigned to the relay control input. Firmware configures it as an
output initially HIGH immediately after HAL initialization, before networking,
and defaults to HIGH on every restart. The dashboard labels it as the door relay
and reports its output latch. The recurring "hello from Embassy" log has been
removed; serial logs report startup, connection changes and relay changes.

Open the dashboard URL from the serial log and use the **Door relay · GPIO26**
button. When the output is HIGH it offers **Set LOW · activate relay**; when LOW
it offers **Set HIGH · release relay**. These activation labels assume an
active-low relay. The button waits for a reading, disables while sending a
command, and displays the state returned by the ESP32. If the response is lost,
the command may have taken effect; the page polls for a fresh reading and does
not automatically resend it.

The selected level is maintained until another command or a board restart,
including if Wi-Fi disconnects or the browser closes. There is no timed pulse
or automatic return to HIGH. Restarting the firmware restores HIGH.

Only GPIO26 is writable. The button sends an explicit, idempotent
`POST /api/relay/high` or `POST /api/relay/low` with header `X-Relay-Control: 1`.
The response is the same GPIO snapshot as `GET /api/gpio`. Ordinary GET requests
never switch the relay. This header blocks ordinary cross-origin browser forms;
it is not authentication.

Use a relay module/driver with a 3.3 V-compatible control input; do not connect
a relay coil directly to the GPIO. HIGH is idle only if your relay is active-low.
Firmware cannot hold the pin HIGH during reset or before startup: use an
external pull-up to 3.3 V (for example 10 kΩ, if suitable for the relay input)
when HIGH must be maintained during those intervals. Check the relay module's
input circuit before wiring; the GPIO must not be pulled up to 5 V.

GPIO26 is an output-capable, non-strapping pin on the original ESP32, separate
from UART0 and flash pins; see Espressif's
[ESP32 GPIO reference](https://github.com/espressif/esp-idf/blob/master/docs/en/api-reference/peripherals/gpio/esp32.inc).
