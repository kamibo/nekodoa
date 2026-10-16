use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Instant, Timer};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_radio::wifi::{
    AuthenticationMethod, Config, ControllerConfig, Interface, WifiController, sta::StationConfig,
};
use static_cell::StaticCell;

const SSID: &str = "las capelles 12";
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

pub fn start(wifi: WIFI<'static>, spawner: Spawner, password: &'static str) -> Stack<'static> {
    let authentication = if password.is_empty() {
        AuthenticationMethod::None
    } else {
        AuthenticationMethod::Wpa2Personal
    };
    let station = StationConfig::default()
        .with_ssid(SSID)
        .with_password(password.into())
        .with_auth_method(authentication);
    let (controller, interfaces) = esp_radio::wifi::new(
        wifi,
        ControllerConfig::default().with_initial_config(Config::Station(station)),
    )
    .expect("Failed to initialize Wi-Fi");

    let rng = Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    let (stack, runner) = embassy_net::new(
        interfaces.station,
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.spawn(connection(controller).expect("Failed to spawn Wi-Fi task"));
    spawner.spawn(network(runner).expect("Failed to spawn network task"));
    stack
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    loop {
        log::info!("Connecting to Wi-Fi: {}", SSID);
        match controller.connect_async().await {
            Ok(info) => {
                let connected_at = Instant::now();
                log::info!(
                    "Wi-Fi connected to {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} on channel {}; waiting for DHCP",
                    info.bssid[0],
                    info.bssid[1],
                    info.bssid[2],
                    info.bssid[3],
                    info.bssid[4],
                    info.bssid[5],
                    info.channel,
                );
                match controller.wait_for_disconnect_async().await {
                    Ok(reason) => log::warn!(
                        "Wi-Fi disconnected after {}s: {:?}",
                        connected_at.elapsed().as_secs(),
                        reason,
                    ),
                    Err(error) => log::warn!("Wi-Fi disconnect wait failed: {:?}", error),
                }
                // Reassociate promptly; a failed attempt below still backs off.
                log::info!("Reconnecting to Wi-Fi immediately");
            }
            Err(error) => {
                log::warn!("Wi-Fi connection failed: {:?}; retrying in 5s", error);
                Timer::after_secs(5).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn network(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await;
}
