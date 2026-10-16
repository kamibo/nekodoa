# nekodoa

![nekodoa logo](src/nekodoa.png)

nekodoa is a small, local-only smart-door controller for an original ESP32
board. It combines Embassy-based Rust firmware, Wi-Fi monitoring, an RFID
allow-list, a DHT sensor, and a relay output in one self-contained dashboard.

## Hardware at a glance

| Function | ESP32 pin | Notes |
| --- | ---: | --- |
| Door relay control | GPIO26 | Active-low; HIGH at boot and when idle |
| DHT sensor data | GPIO27 | 3.3 V, with a 4.7–10 kΩ pull-up if needed |
| WL-134 RFID TX 3.3 V | GPIO16 / UART2 RX | 9600 8N2 |
| RFID UART probe TX | GPIO17 / UART2 TX | Leave disconnected on receive-only readers |
| USB serial console | GPIO1 / UART0 TX | 115200 8N1 |

The reader and relay must have a common ground with the ESP32. Never connect a
5 V UART signal directly to an ESP32 GPIO. The WL-134 reader uses its own
power requirements; follow the reader wiring section below rather than powering
it from the ESP32 3.3 V pin.

Minimal `no_std` Rust firmware using Embassy on an ELEGOO ESP-WROOM-32
development board (original ESP32 / Xtensa). If your board is an ESP32-C3,
C6, or S3, it needs a different target and chip features.

The firmware logs startup and connection changes over UART0, accessible through
the board's USB serial bridge at 115200 baud.
Serial and dashboard log lines include elapsed time since boot in
`LEVEL [HH:MM:SS.mmm] - message` format; hours continue past 24.

Wi-Fi connects to `las capelles 12`, obtains an IPv4 address using DHCP, and
logs connection changes and the assigned address. It reconnects immediately
after a disconnect and waits five seconds after a failed connection attempt.
Connection logs include the access-point BSSID and channel; disconnect logs
include how long the connection lasted and the driver's reason and RSSI.

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
The DHT sampling and RFID receive tasks still run without Wi-Fi.

To build without Wi-Fi credentials:

```sh
cargo build --release --locked
```

For dashboard OTA, first build and install the rollback-enabled bootloader as
shown below. USB installation requires an explicit port and uses this verified
bootloader; the default espflash bootloader does not provide our rollback guarantee.
Once the bootloader artifacts exist, Cargo can install and open the monitor:

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

On the original ESP32, the first HAL UART initialization resets shared UART
buffer memory. Initializing RFID UART2 after console logging has started can
therefore disrupt UART0 output. Startup now configures UART2 first, restores
UART0 at 115200 baud on GPIO1, and only then enables logging and starts tasks.
Both HAL UART handles are kept alive to retain their clock guards; esp-println
remains the sole console writer. The relay is still set HIGH before UART setup.
This startup workaround builds successfully but needs confirmation on hardware.
To check it, boot with reader TX disconnected from GPIO16 and confirm complete,
readable startup lines before reconnecting the reader with power off. RFID
The dashboard counts receive glitches separately. A glitch is a short pulse or
noisy edge on GPIO16, not a decoded RFID byte; the firmware discards its
partial candidate without repeatedly logging it. If glitches increase while
the reader is disconnected, add a short wire and an external 4.7–10 kΩ pull-up
from GPIO16 to 3.3 V. If they continue with the whole reader powered off and
GPIO16 physically unconnected, suspect the board, USB setup or pin damage.

The implementation follows the official
[esp-rs Embassy template](https://github.com/esp-rs/esp-generate/blob/main/template/src/bin/async_main.rs):
`esp-hal` initializes the chip, `esp-rtos` integrates the Embassy executor and
timer driver. Network status logging waits for changes asynchronously.
`ESP_LOG` defaults to `info` in `.cargo/config.toml`; changes to that setting
take effect when rebuilding the firmware.

Wi-Fi initialization and reconnection follow Espressif's
[esp-radio 0.18 DHCP example](https://github.com/esp-rs/esp-hal/blob/esp-radio-v0.18.0/examples/wifi/embassy_dhcp/src/main.rs).

## Dashboard firmware updates and rollback

One USB migration is required before Wi-Fi updates. The target is the original
ESP32 with at least 4 MiB flash. Updates have **no password**: anyone who can
reach the dashboard can install a locally built application. The required
`X-OTA-Control: 1` header prevents ordinary cross-origin forms; it is not authentication.
Firmware embeds the configured Wi-Fi password, so keep exported files private.

The exact layout in `partitions.csv` is checked on-device before enabling OTA:

| Region | Offset | Size |
| --- | --- | --- |
| NVS | `0x9000` | `0x5000` |
| OTA metadata (two sectors) | `0xE000` | `0x2000` |
| `ota_0` | `0x10000` | `0x1E0000` |
| `ota_1` | `0x1F0000` | `0x1E0000` |
| Existing authorization record | `0x3F0000` | `0x1000` |

### Build and initial USB migration

Use ESP-IDF **v5.5.1**, commit `fcae32885b0296b32044cb99ecbdc50d98dddb83`,
with its recommended Xtensa GCC 14.2.0 toolchain. Either activate this IDF
installation (`. /path/to/esp-idf/export.sh`) or install Docker and let the
script use the official `espressif/idf:v5.5.1` image. Build the rollback bootloader
and partition table, then export the Rust app with the existing Wi-Fi credentials:

```sh
# Load the espup environment first if the Xtensa linker is not on PATH.
. "$HOME/export-esp.sh"
tools/build-bootloader.sh
tools/export-ota.sh
```

The IDF project enables `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE` and disables
anti-rollback. No eFuses are burned. The scripts verify the bootloader size and
configuration, partition table contents/MD5, and application size/checksums.
The application-only file is `target/ota/nekodoa.bin`; do not upload an ELF,
bootloader, partition table, merged flash image or multipart form.
Application metadata uses a fixed date/time for repeatable exports. Repeatable
artifacts require the same toolchain, espflash version (tested with 4.5.0),
Cargo.lock, source, credentials, build path and build configuration. The bootloader
script fixes `SOURCE_DATE_EPOCH`. Generated IDF settings and builds are ignored
by Git; remove `bootloader/sdkconfig` before regenerating defaults after configuration changes.

Connect USB, stop other serial monitors, and run:

```sh
tools/install-ota.sh /dev/ttyUSB0
```

This script checks physical flash size, saves a complete 4 MiB backup under
`target/ota/backup-<UTC timestamp>.bin`, and validates that the existing app at
`0x10000` fits the retained slot. It writes the new app to `ota_1`, the partition
table, two OTA metadata records (retained `ota_0` VALID and new `ota_1` NEW), and
finally the custom bootloader. It never erases the whole chip or writes the
sector containing authorized tag IDs and pulse duration. The previous slot is
retained until the next successful OTA upload needs that inactive slot.
The full backup uses 115200 baud and `--max-in-flight 1` by default. Espflash
4.5.0 clears serial input when sending each read acknowledgment; a one-packet
window prevents that from discarding pipelined flash data and causing timeouts.
The script prints a status message every ten seconds. A full backup takes at
least six minutes at 115200 baud; the file is populated when the read completes.
For a USB connection that supports a faster speed, set `ESPFLASH_BAUD=460800`
before the install command (or `cargo run`).
The script is intended for migration or USB recovery; it refuses an erased board
or an existing app too large to preserve. Keep the backup until hardware checks pass.

### Upload and startup behavior

Open the dashboard Overview tab, select `target/ota/nekodoa.bin`, and click
**Upload firmware**. Progress covers transfer, followed by image validation,
restart and reconnection. A changed slot passing startup verification is reported
as success; a return to the previous slot is reported as rollback. A lost upload
response can mean interruption or restart, so the dashboard checks the board
again instead of retrying the upload automatically.

The API accepts `POST /api/ota` with exactly one decimal `Content-Length` and
`X-OTA-Control: 1`; Transfer-Encoding is rejected. Size is limited to the 1,966,080-byte
slot. Upload uses a bounded 1024-byte buffer, sector erases and aligned writes to
only the inactive slot. Headers are limited to 1024 bytes and five seconds;
upload inactivity is limited to ten seconds and total transfer to five minutes.
The app validates the ESP32 chip ID, application descriptor, segment boundaries,
load-address ranges, XOR checksum, and optional image SHA256 before updating boot
selection. It rejects trailing bytes and merged images. No critical section spans
an asynchronous wait. The settings record and OTA share one protected flash owner.

The server rejects concurrent updates, an active LOW relay, an active tag pulse,
or a pending pulse. It atomically blocks relay commands and RFID pulse requests
while an accepted upload owns the update lock. Settings changes receive HTTP 409.
Abandoned/invalid uploads leave boot metadata untouched, clear pending requests
and restore relay operation. An uncertain metadata write triggers a reset with
the relay locked, so the bootloader resolves the last valid selection.

The bootloader tries a NEW app once and changes it to PENDING_VERIFY. The firmware
holds the relay HIGH during verification and arms a 60-second hardware watchdog
before settings, UART and network startup. It confirms the running slot only
after 30 consecutive healthy seconds: relay initialized HIGH, authorization data
loaded (or an erased record selecting defaults), Wi-Fi link and DHCP up, and HTTP
service ready. Corrupt settings, missing network, failed checks or a stalled
startup leave it unconfirmed; a watchdog reset lets the bootloader return to the
previous valid app. On an already confirmed image the startup watchdog is disabled;
normal network reconnection continues without forcing resets. The previous
slot's metadata is preserved during activation and confirmation. See
[ESP-IDF v5.5.1 rollback behavior](https://docs.espressif.com/projects/esp-idf/en/v5.5.1/esp32/api-reference/system/ota.html#app-rollback).

### Verification and USB recovery

Run `tools/export-ota.sh` before host checks, then:

```sh
tools/test-host.sh
# Requires bootloader artifacts; exercises USB writes against a simulated flash.
python3 tests/installation.py
```

Host checks exercise the actual router, image validator, flash-write/activation
logic and relay gate: binary body bytes arriving alongside fragmented headers,
invalid/duplicate lengths, wrong chip, truncation, checksum/hash corruption,
interrupted uploads, concurrent lock attempts, both slot directions and activation
only after complete validation. Dashboard checks cover progress, rejection,
transfer failure, restart, reconnection and rollback messages.

Hardware verification remains required before relying on door operation:

1. Record the tag IDs and pulse duration. Migrate over USB; confirm GPIO26 stays
   HIGH and startup completes with these settings preserved.
2. Upload in both directions, checking slot changes and the 30-second confirmation.
3. Interrupt an upload, including power loss, and verify the previous firmware boots.
4. Upload a test build with deliberately failing startup checks or a stalled task;
   verify reset within 60 seconds, previous-slot boot and settings preservation.
5. During an upload and pending startup, send relay/settings commands and scan an
   authorized tag. Confirm HTTP 409, no LOW pulse and no deferred pulse after failure.
6. Check active/pending pulses reject an upload without changing relay behavior.

If the board cannot return to Wi-Fi, hold BOOT while connecting a USB flasher and
re-run the verified USB installation script. To restore the exact pre-migration
firmware manually, use the complete backup (including authorization data):

```sh
espflash write-bin --chip esp32 --port /dev/ttyUSB0 0 target/ota/backup-<timestamp>.bin
```

That restores the original flash layout and bootloader; reinstall the OTA layout
before using dashboard updates again. USB installation and physical rollback,
power-loss and relay tests cannot be certified by host tests alone.

## WL-134 RFID reader

The reader uses a separate receive-only UART2 on **GPIO16**, configured for
**9600 baud, 8 data bits, no parity, 2 stop bits (8N2)**, without flow control. These
settings come from the supplied reader information and need confirmation with
a real scan. The USB serial console remains **115200 baud** on UART0.
GPIO26 (relay, default HIGH) and GPIO27 (DHT sensor) are unchanged.

Power off before wiring. Based on the labels in the supplied board photo:

| Reader wire / label | ESP-WROOM-32 connection |
| --- | --- |
| Red / `+9V` | Regulated **9 V** supply (about 140 mA); do not use ESP32 3.3 V |
| Black / `GND` | ESP32 GND and supply GND |
| Reader `TX 3.3V` | **GPIO16 directly** (ESP32 UART2 RX) |
| Reader `TX 5V` | **Leave disconnected**; never connect it directly to the ESP32 |
| Reader `RX` (variant only) | Leave disconnected unless a documented command input is needed |
| Two antenna wires | Keep connected to the reader antenna, not ESP32 GPIO |

Use only the reader's dedicated `TX 3.3V` output. Do not connect the 5 V TX
output to GPIO16, and never connect the two TX outputs together. ESP32 GPIO is
not 5 V tolerant (see the [Espressif module electrical specifications](https://documentation.espressif.com/esp32_wroom-32_datasheet_en.pdf)).
Ensure the 9 V supply has enough current for the reader alongside Wi-Fi; use a
suitable separate regulated 9 V supply with common ground if necessary.
Do not tie two independent 3.3 V supply outputs together. This GPIO16 assignment
is for ESP-WROOM-32, not modules that reserve that pin for PSRAM.

The WL-134 decoder accepts **26 ASCII hexadecimal characters**, followed by two
checksum bytes and ETX (`03`):
The 26 data bytes are ASCII hex (`0–9`, `A–F`; lowercase is normalized to
uppercase):

- STX (`02`) starts a frame; the parser requires all 26 data characters, two
  checksum bytes, and ETX (`03`).
- Short, oversized, non-hexadecimal, or incomplete frames are rejected. Parsing
  works across UART reads and handles consecutive framed messages.

This is the candidate format supplied by the user, **not yet verified against
the module or its manual**. The two checksum bytes are validated as XOR and
bitwise complement of the 26 ASCII data bytes. The first ten characters encode
the animal ID and the next four encode the country; both fields are reversed
before conversion, then combined as `country × 1,000,000,000,000 + animal`.
For example, `50997D4CE3AF00...` becomes `250269590436101`. Authorized tags
trigger the configured relay pulse; unauthorized tags never operate the relay.

The firmware sends one `0x00` probe byte every second at 9600 8N2 from GPIO17.
Leave GPIO17 disconnected for the standard receive-only WL-134; this probe is
only relevant if your particular board documents an RX/command input. `0x00` is
not claimed to be a valid WL-134 command.

The web page and `GET /api/gpio` show an `rfid` object containing the last
candidate `code`, `code_age_seconds`, and `codes` report count, plus raw `hex`,
an `ascii` preview, capture age, capture count, UART error count and status.
It also reports `tag_id`, whether the last tag is `authorized`, and the
`authorized_ids` list. The list starts with `250269590436101` and is stored in
a dedicated 4 KiB flash sector, so it survives reset and power loss. The
dashboard can add or delete decimal IDs (up to eight entries total); these
operations use POST requests with `X-Authorized-Control: 1` and `?id=`.
Flash access uses `esp-storage` with critical sections and word-aligned buffers.
The OTA partition table explicitly reserves sector `0x3F0000` for this record.
Do not allocate it to another use. Each
change rewrites the sector, so this is intended for occasional list management
rather than rapid automated updates.

When a decoded RFID number is authorized, the firmware logs an INFO message
and drives the active-low door relay LOW for the configured duration, then
returns it HIGH. The default is 10 seconds. The Overview tab changes this
duration from 1–300 seconds and stores it in the same persistent flash record.
Unauthorized tags produce a WARN log containing the decoded number and never
activate the relay.
Counts include repeated reports; they are not counts of unique tags.
The last code remains visible with its age until reset, even if later input is
invalid or the reader is disconnected. `listening` means the UART is configured,
not that a reader has been detected. Browser network loss clears the display.

Raw captures are grouped by a roughly 50 ms idle gap or split at 128 bytes;
these are diagnostic boundaries, not validated packet boundaries. ASCII dots
represent control/non-ASCII bytes; hex preserves the exact bytes. UART errors
discard the in-progress capture and reset the decoder. Raw capture state is
bounded, and parsing does not operate GPIO outputs. A continuous stream can
also produce one `RFID UART buffer read` log for every successful HAL read;
these logs show the exact buffer chunks before parsing and can be frequent.
Codes are visible to devices that can
access this unauthenticated local dashboard and to anyone reading serial logs.

After flashing, scan a compatible tag and look for:

```text
RFID raw (unvalidated, ...): 02 30 31 ... 03
RFID candidate code (unverified): ...
```

Share a full raw line if no candidate appears or to confirm the message format.
If no bytes appear, check common ground, TX-to-RX wiring, supply, antenna and
tag compatibility. Some firmware may need a command to start reading; no
commands are sent until that protocol is known. For UART errors or gibberish,
check logic voltage and the 9600 8N2 setting. The WL-134 TX is specified as
TTL 5 V, so do not connect it directly to ESP32 GPIO16; use a level shifter or
resistor divider. The WL-134 also requires 9 V power, not 3.3 V. No hardware
scan has yet been tested with this firmware.

Host-side RFID checks:

```sh
rustc +stable --edition=2024 --test src/auth_storage.rs -o /tmp/nekodoa-auth-tests
/tmp/nekodoa-auth-tests
rustc +stable --edition=2024 --test src/rfid_frame.rs -o /tmp/nekodoa-rfid-frame-tests
/tmp/nekodoa-rfid-frame-tests
rustc +stable --edition=2024 --test src/rfid_capture.rs -o /tmp/nekodoa-rfid-capture-tests
/tmp/nekodoa-rfid-capture-tests
node tests/dashboard.mjs
```

## GPIO web page

After flashing, open the `GPIO dashboard: http://…/` address printed in the
serial monitor from a device on the same Wi-Fi network. The ESP32 serves the
page on port 80, with no internet connection or external assets required.
The page polls `GET /api/gpio` once per second and marks old readings as stale
when the board is unreachable. Wi-Fi reconnects automatically. The logo is
served locally from `/nekodoa.png`; no internet connection is required.

The page is organized into Overview, RFID & IDs, GPIO and Logs tabs. Authorized
IDs are shown in a table with a delete action; the add form is in the same tab.

The same response includes `logs`, a 32-line ring buffer of recent INFO, WARN
and ERROR records. The dashboard displays these under **Recent logs**; when
full, the oldest line is discarded. Logs are volatile and are not stored in
flash.

All GPIO numbers 0–39 are listed with a digital level, configuration and usage
notes. Missing, unexposed and flash pins are marked unavailable/reserved.
The monitor only reads registers: it does not enable inputs, change pulls,
or drive pins. Disabled digital inputs and RTC/analog pins show unavailable;
software outputs may show a separately labelled output latch value.
Floating inputs can change randomly, and these samples do not measure voltage
or capture fast pulses. The page also provides a dedicated control for GPIO26;
all other GPIOs remain read-only. Anyone able to reach the ESP32 on your network
can use the relay control; the page has no login.

## Temperature and humidity (DHT11 / DHT22)

The sensor data line uses **GPIO27**, separate from the relay on GPIO26.
Wire it while the board is powered off:

| Sensor connection | ESP32 connection |
| --- | --- |
| VCC / VDD | 3.3 V |
| DATA / SDA | GPIO27 |
| GND | GND |
| NC (bare four-pin sensor only) | Leave disconnected |

Add a 4.7–10 kΩ pull-up resistor from DATA to **3.3 V**, unless the sensor module
already includes one. Use the sensor/module pin labels to identify connections;
three-pin modules can have different connector orders. Do not pull DATA up to
5 V. Keep the sensor cable short.

The current ARCELI module was sold as a DHT22, but its captured frames
`2F 00 1D 01 4D` and `2F 00 1D 00 4C` have valid checksums and match DHT11's
integer/fraction format: **47.0% RH and 29.1 / 29.0 °C**. This is evidence about
its transmitted format, not definitive identification of the hardware inside.
`.cargo/config.toml` therefore sets `DHT_MODEL = "DHT11"` for this installation.
There is no automatic fallback based on plausible values; the selected format
is used consistently and is shown in the dashboard and startup log.

For a sensor that sends actual DHT22/AM2302 frames, set `DHT_MODEL = "DHT22"`
in the `[env]` section of `.cargo/config.toml` (or `wifi.toml`), then rebuild and
flash. An environment variable also overrides the default:

```sh
DHT_MODEL=DHT22 cargo --config wifi.toml run --release --locked -- --port /dev/ttyUSB0
```

DHT11 uses a 20 ms start pulse; DHT22 uses 2 ms. Both retain a three-second
sampling interval and checksum/range validation. Format handling follows the
[Adafruit DHT implementation](https://github.com/adafruit/DHT-sensor-library/blob/master/DHT.cpp).

After flashing, the dashboard shows temperature in °C and relative humidity in
% RH. The firmware waits two seconds at startup, then reads every three seconds.
The page refreshes every second using cached readings; opening more pages does
not increase the sensor sampling rate. Negative temperatures are supported.

Timeouts, invalid checksums and out-of-range readings show a sensor status and
blank values rather than zero or an old measurement. Readings over ten seconds
old are marked stale. If the board is unreachable, the page blanks the values
and reports offline. Check power, ground, the pull-up and GPIO27 when the sensor
reports a timeout. Serial logs report sensor status changes without a heartbeat.

For sensor errors, the first three failed samples and every tenth consecutive
failure also print diagnostic and timing lines prefixed with the selected model. These include
the received bit count, five raw bytes, calculated/received checksums,
unvalidated decoded values and measured pulse lengths. Share both lines when
troubleshooting an out-of-range reading. Logging happens after the timing-critical
receive window; rejected values are still excluded from the dashboard.

`GET /api/gpio` includes a `sensor` object with `pin`, `model`, `status`, `age_seconds`,
`temperature_c` and `humidity_percent`; invalid values are JSON `null`. The
relay response includes the same sensor information.

The reader uses open-drain signaling and a bounded receive window with interrupts
masked (normally about 4 ms, limited to about 6 ms) to capture the sensor's short
pulses. Other tasks can briefly be delayed during a sample. Protocol timing and
data format follow the [Aosong AM2302 manual](https://files.seeedstudio.com/wiki/Grove-Temperature_and_Humidity_Sensor_Pro/res/AM2302-EN.pdf).

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
including if Wi-Fi disconnects or the browser closes. Authorized RFID scans
also produce a timed active-low pulse; its duration is configurable from the
Overview tab and defaults to 10 seconds. Restarting the firmware restores HIGH.

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
