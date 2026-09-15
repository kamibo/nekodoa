use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::Timer;
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
            Ok(_) => {
                log::info!("Wi-Fi connected; waiting for DHCP");
                match controller.wait_for_disconnect_async().await {
                    Ok(reason) => log::warn!("Wi-Fi disconnected: {:?}", reason),
                    Err(error) => log::warn!("Wi-Fi disconnect wait failed: {:?}", error),
                }
            }
            Err(error) => log::warn!("Wi-Fi connection failed: {:?}", error),
        }
        Timer::after_secs(5).await;
    }
}

#[embassy_executor::task]
async fn network(mut runner: Runner<'static, Interface<'static>>) {
    runner.run().await;
}
