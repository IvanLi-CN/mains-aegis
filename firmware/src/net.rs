#![cfg(feature = "net_http")]

use alloc::string::String as AllocString;
use core::{
    cell::RefCell,
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use critical_section::Mutex;
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_net::{
    tcp::TcpSocket, Config as NetConfig, DhcpConfig, Ipv4Address, Ipv4Cidr, Stack, StackResources,
    StaticConfigV4,
};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::WIFI, rng::Rng};
use esp_radio::{
    init as radio_init,
    wifi::{self, ClientConfig, ModeConfig, WifiController, WifiDevice},
    Controller as RadioController,
};
use heapless::{String, Vec};
use static_cell::StaticCell;

use crate::{
    mdns::{self, MdnsRuntimeConfig},
    mdns_wire::{derive_device_identity, DeviceIdentity},
    net_contract::{
        accepts_event_stream, is_api_v1_path, render_charge_control_result_json,
        render_diag_snapshot_json, render_identity_json, render_network_json, render_ping_json,
        render_settings_json, render_status_json, write_error_body, write_json_string_escaped,
        write_sse_event, BuildInfo,
    },
    net_logic::{
        build_chunked_json_response_head, build_http_response_head, build_sse_response_head,
        lan_advanced_power_apply_timeout_ms, origin_reflection_allowed, resolve_net_env_config,
        select_active_dns, LAN_ADVANCED_POWER_APPLY_POLL_INTERVAL_MS,
    },
    net_types::{
        AdvancedPowerCapabilitiesSnapshot, AdvancedPowerSettingsSnapshot,
        ChargeControlDetailSnapshot, DerivedPowerSnapshot, DeviceSettingsSnapshot,
        FrontPanelRuntimeSnapshot, ManualChargeSettingsSnapshot, NetworkUiSummary,
        UpsStatusSnapshot, WifiConnectionState, WifiErrorKind, WifiSettingsSnapshot, WifiSnapshot,
    },
    usb_cdc_protocol::{
        parse_http_advanced_power_request, parse_http_log_level_request,
        parse_http_manual_charge_control_request, parse_http_manual_charge_preview_request,
        parse_http_manual_charge_request, parse_http_reset_request, parse_http_wifi_config_request,
        LogLevel, ManualChargeControlCommand, ManualChargePrefsCommand, WifiConfigSecret,
    },
};

const WIFI_HOSTNAME: Option<&str> = option_env!("MAINS_AEGIS_WIFI_HOSTNAME");
const WIFI_STATIC_IP: Option<&str> = option_env!("MAINS_AEGIS_WIFI_STATIC_IP");
const WIFI_NETMASK: Option<&str> = option_env!("MAINS_AEGIS_WIFI_NETMASK");
const WIFI_GATEWAY: Option<&str> = option_env!("MAINS_AEGIS_WIFI_GATEWAY");
const WIFI_DNS: Option<&str> = option_env!("MAINS_AEGIS_WIFI_DNS");

const HTTP_PORT: u16 = 80;
const HTTP_WORKER_COUNT: usize = 3;
pub const HTTP_RESPONSE_BODY_CAP: usize = 3072;
const HTTP_DIAG_SNAPSHOT_BODY_CAP: usize = 8192;
const SSE_FRAME_CAP: usize = 3328;
const REQUEST_BUF_CAP: usize = 1024;
const STATUS_PUSH_INTERVAL: Duration = Duration::from_millis(500);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const RSSI_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const WIFI_CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LAN_ADVANCED_POWER_APPLY_TIMEOUT: Duration =
    Duration::from_millis(lan_advanced_power_apply_timeout_ms());
const LAN_RECOVERY_TIMEOUT: Duration = Duration::from_secs(70);
const LAN_ADVANCED_POWER_APPLY_POLL_INTERVAL: Duration =
    Duration::from_millis(LAN_ADVANCED_POWER_APPLY_POLL_INTERVAL_MS);

static STATUS_SSE_ACTIVE: AtomicBool = AtomicBool::new(false);
static RADIO_CONTROLLER: StaticCell<RadioController<'static>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
static WIFI_STATE: Mutex<RefCell<WifiSnapshot>> =
    Mutex::new(RefCell::new(WifiSnapshot::disabled()));
static UPS_STATUS: Mutex<RefCell<UpsStatusSnapshot>> =
    Mutex::new(RefCell::new(UpsStatusSnapshot::empty()));
static DIAG_SNAPSHOT: Mutex<RefCell<DerivedPowerSnapshot>> =
    Mutex::new(RefCell::new(DerivedPowerSnapshot::empty()));
static CHARGE_CONTROL_DETAIL: Mutex<RefCell<ChargeControlDetailSnapshot>> =
    Mutex::new(RefCell::new(ChargeControlDetailSnapshot::empty()));
static FRONT_PANEL_RUNTIME: Mutex<RefCell<FrontPanelRuntimeSnapshot>> =
    Mutex::new(RefCell::new(FrontPanelRuntimeSnapshot::unavailable()));
static DEVICE_IDENTITY: Mutex<RefCell<Option<DeviceIdentity>>> = Mutex::new(RefCell::new(None));
static USB_WIFI_CONFIG: Mutex<RefCell<Option<WifiConfigSecret>>> = Mutex::new(RefCell::new(None));
static DEVICE_SETTINGS: Mutex<RefCell<Option<DeviceSettingsSnapshot>>> =
    Mutex::new(RefCell::new(None));
static PENDING_LAN_COMMAND: Mutex<RefCell<Option<LanManagementCommand>>> =
    Mutex::new(RefCell::new(None));
static LAN_COMMAND_RESULT: Mutex<RefCell<Option<LanCommandResult>>> =
    Mutex::new(RefCell::new(None));
static WIFI_CONFIG_GENERATION: AtomicU32 = AtomicU32::new(0);
static DIAG_CAPTURE_BUSY: AtomicBool = AtomicBool::new(false);
static DIAG_CAPTURE_GENERATION: AtomicU32 = AtomicU32::new(0);
static DIAG_CAPTURE_REQUEST_MASK: AtomicU32 = AtomicU32::new(0);
static DIAG_CAPTURE_REQUEST_GENERATION: AtomicU32 = AtomicU32::new(0);
static DIAG_CAPTURE_COMPLETE_GENERATION: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCaptureRequest {
    pub generation: u32,
    pub package_mask: u32,
}

const BUILD_INFO: BuildInfo = BuildInfo {
    package_version: env!("CARGO_PKG_VERSION"),
    build_profile: env!("FW_BUILD_PROFILE"),
    build_id: env!("FW_BUILD_ID"),
    git_sha: env!("FW_GIT_SHA"),
    src_hash: env!("FW_SRC_HASH"),
    git_dirty: env!("FW_GIT_DIRTY"),
    features: env!("FW_FEATURES"),
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanManagementCommand {
    SetWifi(WifiConfigSecret),
    ClearWifi,
    SetLogLevel(LogLevel),
    SetManualCharge(ManualChargePrefsCommand),
    PreviewChargeControl(ManualChargePrefsCommand),
    ControlManualCharge(ManualChargeControlCommand),
    SetAdvancedPower(AdvancedPowerSettingsSnapshot),
    ResetAdvancedPower,
    RecoverBmsDischargeAuthorization,
    Reset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LanCommandResult {
    Ok,
    Json(String<HTTP_RESPONSE_BODY_CAP>),
    AdvancedPowerValidation {
        code: &'static str,
        message: &'static str,
    },
    AdvancedPowerStorageFailed,
    ManualChargeControlError {
        code: &'static str,
        message: &'static str,
        details: String<HTTP_RESPONSE_BODY_CAP>,
    },
}

pub fn publish_ups_status(snapshot: UpsStatusSnapshot) {
    critical_section::with(|cs| {
        *UPS_STATUS.borrow_ref_mut(cs) = snapshot;
    });
}

pub fn publish_diag_snapshot(snapshot: DerivedPowerSnapshot) {
    critical_section::with(|cs| {
        *DIAG_SNAPSHOT.borrow_ref_mut(cs) = snapshot;
    });
}

pub fn take_diag_capture_request() -> Option<DiagCaptureRequest> {
    let generation = DIAG_CAPTURE_REQUEST_GENERATION.swap(0, Ordering::AcqRel);
    (generation != 0).then(|| DiagCaptureRequest {
        generation,
        package_mask: DIAG_CAPTURE_REQUEST_MASK.load(Ordering::Acquire),
    })
}

pub fn complete_diag_capture(request: DiagCaptureRequest, snapshot: DerivedPowerSnapshot) {
    publish_diag_snapshot(snapshot);
    DIAG_CAPTURE_COMPLETE_GENERATION.store(request.generation, Ordering::Release);
    DIAG_CAPTURE_BUSY.store(false, Ordering::Release);
}

pub fn publish_charge_control_detail(snapshot: ChargeControlDetailSnapshot) {
    critical_section::with(|cs| {
        *CHARGE_CONTROL_DETAIL.borrow_ref_mut(cs) = snapshot;
    });
}

pub fn current_network_ui_summary() -> NetworkUiSummary {
    NetworkUiSummary::from_wifi(current_wifi_snapshot())
}

pub fn current_wifi_snapshot() -> WifiSnapshot {
    critical_section::with(|cs| *WIFI_STATE.borrow_ref(cs))
}

pub fn current_identity() -> Option<DeviceIdentity> {
    critical_section::with(|cs| DEVICE_IDENTITY.borrow_ref(cs).clone())
}

pub fn current_device_settings() -> DeviceSettingsSnapshot {
    critical_section::with(|cs| {
        DEVICE_SETTINGS
            .borrow_ref(cs)
            .clone()
            .unwrap_or_else(DeviceSettingsSnapshot::defaults)
    })
}

pub fn take_pending_lan_command() -> Option<LanManagementCommand> {
    critical_section::with(|cs| PENDING_LAN_COMMAND.borrow_ref_mut(cs).take())
}

pub fn set_lan_command_result(result: LanCommandResult) {
    critical_section::with(|cs| {
        *LAN_COMMAND_RESULT.borrow_ref_mut(cs) = Some(result);
    });
}

fn take_lan_command_result() -> Option<LanCommandResult> {
    critical_section::with(|cs| LAN_COMMAND_RESULT.borrow_ref_mut(cs).take())
}

fn queue_lan_command(command: LanManagementCommand) -> Result<(), ()> {
    critical_section::with(|cs| {
        let mut pending = PENDING_LAN_COMMAND.borrow_ref_mut(cs);
        if pending.is_some() {
            Err(())
        } else {
            *LAN_COMMAND_RESULT.borrow_ref_mut(cs) = None;
            *pending = Some(command);
            Ok(())
        }
    })
}

pub fn log_wifi_config() {
    esp_println::println!(
        "net: feature=net_http default_wifi=disabled static_ip={:?} hostname_override={:?}",
        WIFI_STATIC_IP,
        WIFI_HOSTNAME
    );
    info!(
        "net: feature=net_http default_wifi=disabled static_ip={:?} hostname_override={:?}",
        WIFI_STATIC_IP, WIFI_HOSTNAME,
    );
}

pub fn set_usb_wifi_config(config: Option<WifiConfigSecret>) {
    critical_section::with(|cs| {
        *USB_WIFI_CONFIG.borrow_ref_mut(cs) = config;
        let mut settings = DEVICE_SETTINGS
            .borrow_ref(cs)
            .clone()
            .unwrap_or_else(DeviceSettingsSnapshot::defaults);
        settings.wifi = match USB_WIFI_CONFIG.borrow_ref(cs).as_ref() {
            Some(secret) => WifiSettingsSnapshot {
                configured: true,
                ssid: Some(secret.ssid.clone()),
            },
            None => WifiSettingsSnapshot::unconfigured(),
        };
        *DEVICE_SETTINGS.borrow_ref_mut(cs) = Some(settings);
    });
    WIFI_CONFIG_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn current_usb_wifi_config() -> Option<WifiConfigSecret> {
    critical_section::with(|cs| USB_WIFI_CONFIG.borrow_ref(cs).clone())
}

fn wifi_config_generation() -> u32 {
    WIFI_CONFIG_GENERATION.load(Ordering::SeqCst)
}

pub fn set_device_log_level(level: &'static str) {
    critical_section::with(|cs| {
        let mut settings = DEVICE_SETTINGS
            .borrow_ref(cs)
            .clone()
            .unwrap_or_else(DeviceSettingsSnapshot::defaults);
        settings.log_level = level;
        *DEVICE_SETTINGS.borrow_ref_mut(cs) = Some(settings);
    });
}

pub fn set_manual_charge_settings(
    target: &'static str,
    speed: &'static str,
    timer_h: u8,
    power_path: &'static str,
) {
    critical_section::with(|cs| {
        let mut settings = DEVICE_SETTINGS
            .borrow_ref(cs)
            .clone()
            .unwrap_or_else(DeviceSettingsSnapshot::defaults);
        settings.manual_charge = ManualChargeSettingsSnapshot {
            target,
            speed,
            timer_h,
            power_path,
        };
        *DEVICE_SETTINGS.borrow_ref_mut(cs) = Some(settings);
    });
}

pub fn set_advanced_power_settings(
    advanced_power: AdvancedPowerSettingsSnapshot,
    capabilities: AdvancedPowerCapabilitiesSnapshot,
) {
    critical_section::with(|cs| {
        let mut settings = DEVICE_SETTINGS.borrow_ref(cs).clone().unwrap_or_else(|| {
            DeviceSettingsSnapshot::defaults_for_rated_vout(capabilities.rated_vout_mv)
        });
        settings.advanced_power = advanced_power;
        settings.advanced_power_capabilities = capabilities;
        *DEVICE_SETTINGS.borrow_ref_mut(cs) = Some(settings);
    });
}

pub fn spawn_wifi_and_http(
    spawner: &Spawner,
    wifi_peripheral: WIFI<'static>,
    usb_wifi_config: Option<WifiConfigSecret>,
) {
    set_usb_wifi_config(usb_wifi_config);
    esp_println::println!("net: spawn begin");
    info!("net: spawn begin");
    let radio = match radio_init() {
        Ok(radio) => radio,
        Err(err) => {
            esp_println::println!("net: radio init failed");
            warn!("net: radio init failed: {:?}", err);
            return;
        }
    };
    let radio = RADIO_CONTROLLER.init(radio);

    let (controller, interfaces) = match wifi::new(radio, wifi_peripheral, Default::default()) {
        Ok(v) => v,
        Err(err) => {
            esp_println::println!("net: wifi::new failed");
            warn!("net: wifi::new failed: {:?}", err);
            return;
        }
    };

    let wifi_device: WifiDevice<'static> = interfaces.sta;
    let mac = wifi_device.mac_address();
    esp_println::println!(
        "net: wifi device ready mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    let identity = derive_device_identity(mac);
    critical_section::with(|cs| {
        *DEVICE_IDENTITY.borrow_ref_mut(cs) = Some(identity.clone());
        *WIFI_STATE.borrow_ref_mut(cs) = WifiSnapshot {
            state: WifiConnectionState::Connecting,
            mac: Some(mac),
            ..WifiSnapshot::disabled()
        };
    });

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;
    let (net_cfg, is_static, static_cfg_error, configured_dns) = build_net_config_from_env();
    let resources = NET_RESOURCES.init(StackResources::<8>::new());
    let (stack, runner) = embassy_net::new(wifi_device, net_cfg, resources, seed);

    spawner.spawn(net_task(runner)).expect("spawn net_task");
    spawner
        .spawn(wifi_task(
            controller,
            stack,
            is_static,
            configured_dns,
            static_cfg_error,
            mac,
        ))
        .expect("spawn wifi_task");
    spawner
        .spawn(mdns::mdns_task(
            stack,
            MdnsRuntimeConfig {
                identity,
                port: HTTP_PORT,
            },
        ))
        .expect("spawn mdns_task");
    for worker_id in 0..HTTP_WORKER_COUNT {
        spawner
            .spawn(http_worker(stack, worker_id))
            .expect("spawn http_worker");
    }
    esp_println::println!("net: tasks spawned");
    info!("net: tasks spawned");
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, WifiDevice<'static>>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn wifi_task(
    mut controller: WifiController<'static>,
    stack: Stack<'static>,
    is_static: bool,
    configured_dns: Option<[u8; 4]>,
    static_cfg_error: Option<WifiErrorKind>,
    mac: [u8; 6],
) {
    let mut backoff_secs = 2u64;
    let mut configured_generation: Option<u32> = None;
    esp_println::println!("net: wifi task start");
    info!("net: wifi task start");
    loop {
        let credential_generation = wifi_config_generation();
        let usb_wifi_config = current_usb_wifi_config();
        let config_changed = configured_generation != Some(credential_generation);
        let Some(config) = usb_wifi_config.as_ref() else {
            set_wifi_snapshot(WifiSnapshot {
                mac: Some(mac),
                ..WifiSnapshot::disabled()
            });
            if matches!(controller.is_connected(), Ok(true)) {
                esp_println::println!("net: wifi config cleared disconnect");
                let _ = controller.disconnect_async().await;
            }
            if matches!(controller.is_started(), Ok(true)) {
                esp_println::println!("net: wifi config cleared stop");
                let _ = controller.stop_async().await;
            }
            configured_generation = Some(credential_generation);
            Timer::after(Duration::from_millis(500)).await;
            continue;
        };
        let ssid = config.ssid.as_str();
        let psk = config.psk.as_str();

        set_wifi_snapshot(WifiSnapshot {
            state: WifiConnectionState::Connecting,
            gateway: None,
            ipv4: None,
            dns: configured_dns,
            is_static,
            last_error: static_cfg_error,
            rssi_dbm: None,
            mac: Some(mac),
        });

        let client_config = ModeConfig::Client(
            ClientConfig::default()
                .with_ssid(AllocString::from(ssid))
                .with_password(AllocString::from(psk)),
        );

        if config_changed && matches!(controller.is_connected(), Ok(true)) {
            esp_println::println!("net: wifi credential change disconnect");
            let _ = controller.disconnect_async().await;
        }
        if config_changed && matches!(controller.is_started(), Ok(true)) {
            esp_println::println!("net: wifi credential change stop");
            let _ = controller.stop_async().await;
        }

        if config_changed || !matches!(controller.is_started(), Ok(true)) {
            esp_println::println!("net: wifi set_config");
            if let Err(err) = controller.set_config(&client_config) {
                esp_println::println!("net: set_config failed");
                warn!("net: set_config failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
                continue;
            }
            configured_generation = Some(credential_generation);
            esp_println::println!("net: wifi start_async begin");
            if let Err(err) = controller.start_async().await {
                esp_println::println!("net: start_async failed");
                warn!("net: start_async failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
                continue;
            }
            esp_println::println!("net: wifi start_async ok");
        }

        esp_println::println!("net: connecting to ssid={} source={}", ssid, "eeprom");
        info!("net: connecting to ssid={} source={}", ssid, "eeprom");
        match controller.connect_async().await {
            Ok(()) => {
                esp_println::println!("net: connect_async ok");
                let mut ready = false;
                for _ in 0..30 {
                    if stack.is_config_up() {
                        ready = true;
                        break;
                    }
                    Timer::after(Duration::from_millis(500)).await;
                }
                if !ready {
                    esp_println::println!("net: config wait timeout");
                    note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::DhcpTimeout);
                    Timer::after(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = backoff_secs.saturating_mul(2).min(30);
                    continue;
                }
                if let Some(v4) = stack.config_v4() {
                    let ip = v4.address.address().octets();
                    let gateway = v4.gateway.map(|value| value.octets());
                    let mut runtime_dns = [[0u8; 4]; 3];
                    let mut runtime_dns_len = 0usize;
                    for dns_server in v4.dns_servers.iter() {
                        if runtime_dns_len >= runtime_dns.len() {
                            break;
                        }
                        runtime_dns[runtime_dns_len] = dns_server.octets();
                        runtime_dns_len += 1;
                    }
                    let dns = select_active_dns(configured_dns, &runtime_dns[..runtime_dns_len]);
                    let rssi_dbm = read_controller_rssi(&controller);
                    set_wifi_snapshot(WifiSnapshot {
                        state: WifiConnectionState::Connected,
                        ipv4: Some(ip),
                        gateway,
                        dns,
                        is_static,
                        last_error: static_cfg_error,
                        rssi_dbm,
                        mac: Some(mac),
                    });
                    backoff_secs = 2;
                    info!(
                        "net: wifi connected ip={}.{}.{}.{}",
                        ip[0], ip[1], ip[2], ip[3]
                    );
                    esp_println::println!(
                        "net: wifi connected ip={}.{}.{}.{}",
                        ip[0],
                        ip[1],
                        ip[2],
                        ip[3]
                    );
                }

                let mut disconnected_for_config_change = false;
                let mut rssi_refresh_elapsed = Duration::from_millis(0);
                loop {
                    Timer::after(WIFI_CONFIG_POLL_INTERVAL).await;
                    rssi_refresh_elapsed += WIFI_CONFIG_POLL_INTERVAL;
                    let next_generation = wifi_config_generation();
                    if next_generation != credential_generation {
                        if current_usb_wifi_config().is_none() {
                            esp_println::println!("net: wifi config cleared disconnect");
                            set_wifi_snapshot(WifiSnapshot {
                                mac: Some(mac),
                                ..WifiSnapshot::disabled()
                            });
                            let _ = controller.disconnect_async().await;
                            if matches!(controller.is_started(), Ok(true)) {
                                esp_println::println!("net: wifi config cleared stop");
                                let _ = controller.stop_async().await;
                            }
                            configured_generation = Some(next_generation);
                        } else {
                            esp_println::println!("net: wifi credential change reconnect");
                            let _ = controller.disconnect_async().await;
                        }
                        disconnected_for_config_change = true;
                        break;
                    }
                    if !matches!(controller.is_connected(), Ok(true)) {
                        break;
                    }
                    if rssi_refresh_elapsed < RSSI_REFRESH_INTERVAL {
                        continue;
                    }
                    rssi_refresh_elapsed = Duration::from_millis(0);
                    let rssi_dbm = read_controller_rssi(&controller);
                    let snapshot = current_wifi_snapshot();
                    set_wifi_snapshot(WifiSnapshot {
                        rssi_dbm,
                        mac: Some(mac),
                        ..snapshot
                    });
                }
                if disconnected_for_config_change {
                    Timer::after(Duration::from_millis(100)).await;
                    continue;
                }
                esp_println::println!("net: wifi disconnected");
                warn!("net: wifi disconnected");
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::LinkLost);
                Timer::after(Duration::from_secs(3)).await;
            }
            Err(err) => {
                esp_println::println!("net: connect_async failed");
                warn!("net: connect_async failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
            }
        }
    }
}

fn read_controller_rssi(controller: &WifiController<'static>) -> Option<i8> {
    controller
        .rssi()
        .ok()
        .and_then(|rssi| i8::try_from(rssi).ok())
}

#[embassy_executor::task(pool_size = HTTP_WORKER_COUNT)]
async fn http_worker(stack: Stack<'static>, worker_id: usize) {
    let mut rx_buf = [0u8; REQUEST_BUF_CAP];
    let mut tx_buf = [0u8; REQUEST_BUF_CAP];
    info!("net: http worker {} ready port={}", worker_id, HTTP_PORT);

    loop {
        stack.wait_config_up().await;
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(10)));
        match socket.accept(HTTP_PORT).await {
            Ok(()) => {
                if let Err(err) = handle_http_connection(&mut socket).await {
                    warn!("net: http worker {} error: {:?}", worker_id, err);
                }
                socket.close();
                let _ = socket.flush().await;
            }
            Err(err) => {
                warn!("net: http accept error worker={} err={:?}", worker_id, err);
                Timer::after(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn handle_http_connection(socket: &mut TcpSocket<'_>) -> Result<(), embassy_net::tcp::Error> {
    let mut buf = [0u8; REQUEST_BUF_CAP];
    let mut total = 0usize;
    loop {
        let read = socket.read(&mut buf[total..]).await?;
        if read == 0 {
            break;
        }
        total += read;
        if total >= buf.len() || buf[..total].windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    if total == 0 {
        return Ok(());
    }

    let req = match core::str::from_utf8(&buf[..total]) {
        Ok(req) => req,
        Err(_) => {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(
                &mut body,
                "invalid_request",
                "request is not valid utf-8",
                false,
                None,
            );
            write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
            return Ok(());
        }
    };

    let Some(header_end) = req.find("\r\n\r\n") else {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "invalid_request",
            "malformed http headers",
            false,
            None,
        );
        write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
        return Ok(());
    };

    let mut lines = req[..header_end].lines();
    let request_line = lines.next().unwrap_or("");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("");
    let path = request_parts.next().unwrap_or("");
    let version = request_parts.next().unwrap_or("HTTP/1.1");
    if version != "HTTP/1.1" {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "invalid_request",
            "only http/1.1 is supported",
            false,
            None,
        );
        write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
        return Ok(());
    }

    let mut method_buf = String::<8>::new();
    let mut path_buf = String::<128>::new();
    if method_buf.push_str(method).is_err() || path_buf.push_str(path).is_err() {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "invalid_request",
            "request line is too long",
            false,
            None,
        );
        write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
        return Ok(());
    }

    let mut origin: Option<String<128>> = None;
    let mut accept_sse = false;
    let mut content_length = 0usize;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("origin:") {
            if let Some(value) = line
                .split_once(':')
                .map(|(_, value)| value.trim())
                .filter(|value| !value.is_empty())
            {
                let mut origin_buf = String::<128>::new();
                if origin_buf.push_str(value).is_err() {
                    let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
                    write_error_body(
                        &mut body,
                        "invalid_request",
                        "origin header is too long",
                        false,
                        None,
                    );
                    write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
                    return Ok(());
                }
                origin = Some(origin_buf);
            }
        } else if lower.starts_with("accept:") {
            if let Some((_, value)) = line.split_once(':') {
                accept_sse = accepts_event_stream(value.trim());
            }
        } else if lower.starts_with("content-length:") {
            content_length = line
                .split_once(':')
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(usize::MAX);
        }
    }

    let origin = origin.as_ref().map(|value| value.as_str());
    let method = method_buf.as_str();
    let request_target = path_buf.as_str();
    let (path, query) = split_request_target(request_target);

    if let Some(value) = origin {
        if !origin_reflection_allowed(value) {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(
                &mut body,
                "invalid_request",
                "origin header is too long",
                false,
                None,
            );
            write_http_response(socket, "400 Bad Request", body.as_str(), None).await?;
            return Ok(());
        }
    }

    let body_start = header_end + 4;
    let request_len = match body_start.checked_add(content_length) {
        Some(request_len) if request_len <= buf.len() => request_len,
        _ => {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(
                &mut body,
                "invalid_request",
                "request body is too large",
                false,
                None,
            );
            write_http_response(socket, "413 Payload Too Large", body.as_str(), origin).await?;
            return Ok(());
        }
    };
    while total < request_len {
        let read = socket.read(&mut buf[total..request_len]).await?;
        if read == 0 {
            break;
        }
        total += read;
    }
    if total < request_len {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "invalid_request",
            "request body is incomplete",
            false,
            None,
        );
        write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
        return Ok(());
    }
    let req = match core::str::from_utf8(&buf[..request_len]) {
        Ok(req) => req,
        Err(_) => {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(
                &mut body,
                "invalid_request",
                "request body is not valid utf-8",
                false,
                None,
            );
            write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
            return Ok(());
        }
    };
    let request_body = &req[body_start..request_len];

    if method == "OPTIONS" {
        if is_api_v1_path(path) {
            write_http_response(socket, "200 OK", "", origin).await?;
        } else {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(&mut body, "not_found", "not found", false, None);
            write_http_response(socket, "404 Not Found", body.as_str(), origin).await?;
        }
        return Ok(());
    }

    if method == "POST" || method == "DELETE" {
        handle_http_write(socket, method, path, request_body, origin).await?;
        return Ok(());
    }

    if method != "GET" {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "invalid_request",
            "only get and options are supported",
            false,
            None,
        );
        write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
        return Ok(());
    }

    let identity = match current_identity() {
        Some(identity) => identity,
        None => {
            let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
            write_error_body(&mut body, "unavailable", "identity not ready", true, None);
            write_http_response(socket, "503 Service Unavailable", body.as_str(), origin).await?;
            return Ok(());
        }
    };

    let wifi = current_wifi_snapshot();
    let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
    match path {
        "/api/v1/ping" | "/health" => {
            render_ping_json(&mut body);
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/identity" => {
            render_identity_json(&mut body, &identity, wifi, BUILD_INFO);
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/network" => {
            render_network_json(&mut body, &identity, wifi);
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/settings" => {
            let settings = current_device_settings();
            render_settings_json(&mut body, &settings);
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/status" if accept_sse => {
            handle_status_sse(socket, origin).await?;
        }
        "/api/v1/status" => {
            render_status_json(&mut body, current_status_snapshot());
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/charge-control" => {
            render_charge_control_result_json(&mut body, current_charge_control_detail());
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        "/api/v1/diag-snapshot" => {
            let mut diag_body = String::<HTTP_DIAG_SNAPSHOT_BODY_CAP>::new();
            let packages = parse_diag_snapshot_query_packages(query);
            if let Err(code) = request_diag_capture(packages.as_slice()).await {
                let mut error_body = String::<HTTP_RESPONSE_BODY_CAP>::new();
                write_error_body(
                    &mut error_body,
                    code,
                    if code == "diag_capture_busy" {
                        "another diagnostic capture is already in progress"
                    } else {
                        "diagnostic capture did not complete within 10 seconds"
                    },
                    true,
                    None,
                );
                write_http_response(
                    socket,
                    if code == "diag_capture_busy" {
                        "409 Conflict"
                    } else {
                        "504 Gateway Timeout"
                    },
                    error_body.as_str(),
                    origin,
                )
                .await?;
                return Ok(());
            }
            write_diag_chunked_response(
                socket,
                packages.as_slice(),
                current_status_snapshot(),
                current_diag_snapshot(),
                origin,
                &mut diag_body,
            )
            .await?;
        }
        _ => {
            write_error_body(&mut body, "not_found", "not found", false, None);
            write_http_response(socket, "404 Not Found", body.as_str(), origin).await?;
        }
    }

    Ok(())
}

async fn handle_http_write(
    socket: &mut TcpSocket<'_>,
    method: &str,
    path: &str,
    request_body: &str,
    origin: Option<&str>,
) -> Result<(), embassy_net::tcp::Error> {
    let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
    let mut await_command_result = false;
    let mut await_command_timeout = LAN_ADVANCED_POWER_APPLY_TIMEOUT;
    let queued = match (method, path) {
        ("POST", "/api/v1/wifi-config") => match parse_http_wifi_config_request(request_body) {
            Ok(secret) => queue_lan_command(LanManagementCommand::SetWifi(secret)),
            Err(err) => {
                write_error_body(&mut body, err.code(), err.message(), false, None);
                write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                return Ok(());
            }
        },
        ("DELETE", "/api/v1/wifi-config") => queue_lan_command(LanManagementCommand::ClearWifi),
        ("POST", "/api/v1/settings/log-level") => {
            match parse_http_log_level_request(request_body) {
                Ok(level) => queue_lan_command(LanManagementCommand::SetLogLevel(level)),
                Err(err) => {
                    write_error_body(&mut body, err.code(), err.message(), false, None);
                    write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                    return Ok(());
                }
            }
        }
        ("POST", "/api/v1/settings/manual-charge") => {
            match parse_http_manual_charge_request(request_body) {
                Ok(prefs) => queue_lan_command(LanManagementCommand::SetManualCharge(prefs)),
                Err(err) => {
                    write_error_body(&mut body, err.code(), err.message(), false, None);
                    write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                    return Ok(());
                }
            }
        }
        ("POST", "/api/v1/control/manual-charge") => {
            match parse_http_manual_charge_control_request(request_body) {
                Ok(command) => {
                    await_command_result = true;
                    queue_lan_command(LanManagementCommand::ControlManualCharge(command))
                }
                Err(err) => {
                    write_error_body(&mut body, err.code(), err.message(), false, None);
                    write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                    return Ok(());
                }
            }
        }
        ("POST", "/api/v1/charge-control/preview") => {
            match parse_http_manual_charge_preview_request(request_body) {
                Ok(prefs) => {
                    await_command_result = true;
                    queue_lan_command(LanManagementCommand::PreviewChargeControl(prefs))
                }
                Err(err) => {
                    write_error_body(&mut body, err.code(), err.message(), false, None);
                    write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                    return Ok(());
                }
            }
        }
        ("POST", "/api/v1/settings/advanced-power") => {
            match parse_http_advanced_power_request(request_body) {
                Ok(settings) => {
                    await_command_result = true;
                    queue_lan_command(LanManagementCommand::SetAdvancedPower(settings))
                }
                Err(err) => {
                    write_error_body(&mut body, err.code(), err.message(), false, None);
                    write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                    return Ok(());
                }
            }
        }
        ("POST", "/api/v1/settings/advanced-power/reset") => {
            await_command_result = true;
            queue_lan_command(LanManagementCommand::ResetAdvancedPower)
        }
        ("POST", "/api/v1/recovery/bms-discharge-authorization") => {
            await_command_result = true;
            await_command_timeout = LAN_RECOVERY_TIMEOUT;
            queue_lan_command(LanManagementCommand::RecoverBmsDischargeAuthorization)
        }
        ("POST", "/api/v1/reset") => match parse_http_reset_request(request_body) {
            Ok(()) => queue_lan_command(LanManagementCommand::Reset),
            Err(err) => {
                write_error_body(&mut body, err.code(), err.message(), false, None);
                write_http_response(socket, "400 Bad Request", body.as_str(), origin).await?;
                return Ok(());
            }
        },
        _ => {
            write_error_body(&mut body, "not_found", "not found", false, None);
            write_http_response(socket, "404 Not Found", body.as_str(), origin).await?;
            return Ok(());
        }
    };

    if queued.is_err() {
        write_error_body(
            &mut body,
            "busy",
            "another LAN management command is still pending",
            true,
            None,
        );
        write_http_response(socket, "409 Conflict", body.as_str(), origin).await?;
        return Ok(());
    }

    if await_command_result {
        let deadline = embassy_time::Instant::now() + await_command_timeout;
        loop {
            if let Some(result) = take_lan_command_result() {
                match result {
                    LanCommandResult::Ok => break,
                    LanCommandResult::Json(json) => {
                        write_http_response(socket, "200 OK", json.as_str(), origin).await?;
                        return Ok(());
                    }
                    LanCommandResult::AdvancedPowerValidation { code, message } => {
                        write_error_body(&mut body, code, message, false, None);
                        write_http_response(socket, "400 Bad Request", body.as_str(), origin)
                            .await?;
                        return Ok(());
                    }
                    LanCommandResult::AdvancedPowerStorageFailed => {
                        write_error_body(
                            &mut body,
                            "advanced_power_write_failed",
                            "failed to persist advanced power settings",
                            true,
                            None,
                        );
                        write_http_response(
                            socket,
                            "503 Service Unavailable",
                            body.as_str(),
                            origin,
                        )
                        .await?;
                        return Ok(());
                    }
                    LanCommandResult::ManualChargeControlError {
                        code,
                        message,
                        details,
                    } => {
                        write_error_body(&mut body, code, message, false, Some(details.as_str()));
                        let status = if code == "loop_confirmation_required" {
                            "409 Conflict"
                        } else {
                            "400 Bad Request"
                        };
                        write_http_response(socket, status, body.as_str(), origin).await?;
                        return Ok(());
                    }
                }
            }
            if embassy_time::Instant::now() >= deadline {
                write_error_body(
                    &mut body,
                    "lan_command_timeout",
                    "LAN management command did not complete before timeout",
                    true,
                    None,
                );
                write_http_response(socket, "504 Gateway Timeout", body.as_str(), origin).await?;
                return Ok(());
            }
            Timer::after(LAN_ADVANCED_POWER_APPLY_POLL_INTERVAL).await;
        }
    }

    body.clear();
    let _ = body.push_str(r#"{"accepted":true}"#);
    write_http_response(socket, "202 Accepted", body.as_str(), origin).await?;
    Ok(())
}

async fn handle_status_sse(
    socket: &mut TcpSocket<'_>,
    origin: Option<&str>,
) -> Result<(), embassy_net::tcp::Error> {
    if STATUS_SSE_ACTIVE.swap(true, Ordering::AcqRel) {
        let mut body = String::<HTTP_RESPONSE_BODY_CAP>::new();
        write_error_body(
            &mut body,
            "unavailable",
            "status stream already in use",
            true,
            None,
        );
        write_http_response(socket, "409 Conflict", body.as_str(), origin).await?;
        return Ok(());
    }

    let result = async {
        write_sse_response_head(socket, origin).await?;
        let mut next_heartbeat = HEARTBEAT_INTERVAL;
        let mut event_id = 1u32;
        loop {
            let mut status_json = String::<HTTP_RESPONSE_BODY_CAP>::new();
            render_status_json(&mut status_json, current_status_snapshot());
            let mut frame = String::<SSE_FRAME_CAP>::new();
            write_sse_event(&mut frame, "status", status_json.as_str(), Some(event_id));
            event_id = event_id.wrapping_add(1);
            socket_write_all(socket, frame.as_bytes()).await?;
            socket.flush().await?;

            if next_heartbeat <= STATUS_PUSH_INTERVAL {
                let mut heartbeat = String::<64>::new();
                write_sse_event(
                    &mut heartbeat,
                    "heartbeat",
                    r#"{"ok":true}"#,
                    Some(event_id),
                );
                event_id = event_id.wrapping_add(1);
                socket_write_all(socket, heartbeat.as_bytes()).await?;
                socket.flush().await?;
                next_heartbeat = HEARTBEAT_INTERVAL;
            } else {
                next_heartbeat -= STATUS_PUSH_INTERVAL;
            }

            Timer::after(STATUS_PUSH_INTERVAL).await;
        }
        #[allow(unreachable_code)]
        Ok::<(), embassy_net::tcp::Error>(())
    }
    .await;

    STATUS_SSE_ACTIVE.store(false, Ordering::Release);
    result
}

async fn socket_write_all(
    socket: &mut TcpSocket<'_>,
    mut data: &[u8],
) -> Result<(), embassy_net::tcp::Error> {
    while !data.is_empty() {
        let written = socket.write(data).await?;
        if written == 0 {
            return Err(embassy_net::tcp::Error::ConnectionReset);
        }
        data = &data[written..];
    }
    Ok(())
}

async fn write_http_response(
    socket: &mut TcpSocket<'_>,
    status: &str,
    body: &str,
    origin: Option<&str>,
) -> Result<(), embassy_net::tcp::Error> {
    let Some(head) = build_http_response_head(status, body.as_bytes().len(), origin) else {
        return Err(embassy_net::tcp::Error::ConnectionReset);
    };
    socket_write_all(socket, head.as_bytes()).await?;
    socket_write_all(socket, body.as_bytes()).await?;
    Ok(())
}

async fn write_http_chunk(
    socket: &mut TcpSocket<'_>,
    chunk: &str,
) -> Result<(), embassy_net::tcp::Error> {
    let mut header = String::<24>::new();
    let _ = write!(header, "{:X}\r\n", chunk.len());
    socket_write_all(socket, header.as_bytes()).await?;
    socket_write_all(socket, chunk.as_bytes()).await?;
    socket_write_all(socket, b"\r\n").await
}

async fn write_diag_chunked_response(
    socket: &mut TcpSocket<'_>,
    packages: &[String<32>],
    status: UpsStatusSnapshot,
    diag: DerivedPowerSnapshot,
    origin: Option<&str>,
    package_body: &mut String<HTTP_DIAG_SNAPSHOT_BODY_CAP>,
) -> Result<(), embassy_net::tcp::Error> {
    let Some(head) = build_chunked_json_response_head("200 OK", origin) else {
        return Err(embassy_net::tcp::Error::ConnectionReset);
    };
    socket_write_all(socket, head.as_bytes()).await?;
    write_http_chunk(socket, "{\"schema_version\":2,\"packages\":{").await?;

    let iterations = packages.len().max(1);
    let mut emitted = false;
    let mut package_too_large = false;
    for index in 0..iterations {
        let mut single = Vec::<String<32>, 1>::new();
        if let Some(package) = packages.get(index) {
            let _ = single.push(package.clone());
        }
        render_diag_snapshot_json(package_body, single.as_slice(), status, diag);
        let prefix = "{\"schema_version\":2,\"packages\":{";
        let Some(rest) = package_body.as_str().strip_prefix(prefix) else {
            package_too_large = true;
            continue;
        };
        let Some((package_json, _)) = rest.split_once("},\"errors\":{") else {
            package_too_large = true;
            continue;
        };
        if package_json.is_empty() {
            continue;
        }
        if emitted {
            write_http_chunk(socket, ",").await?;
        }
        write_http_chunk(socket, package_json).await?;
        emitted = true;
    }

    write_http_chunk(socket, "},\"errors\":{").await?;
    let mut errors = String::<HTTP_RESPONSE_BODY_CAP>::new();
    let mut first_error = true;
    for package in packages {
        if diag_package_supported(package.as_str()) {
            continue;
        }
        if !first_error {
            let _ = errors.push(',');
        }
        first_error = false;
        let _ = errors.push('"');
        write_json_string_escaped(&mut errors, package.as_str());
        let _ = errors.push_str("\":{\"code\":\"unsupported_package\",\"message\":\"diagnostic package is not supported\"}");
    }
    if let Some(code) = diag.hardware.capture_error {
        if !first_error {
            let _ = errors.push(',');
        }
        first_error = false;
        let _ = errors.push_str("\"capture\":{\"code\":\"");
        write_json_string_escaped(&mut errors, code);
        let _ = write!(
            errors,
            "\",\"retryable\":true,\"retry_after_ms\":{}}}",
            diag.hardware.retry_after_ms.unwrap_or(0)
        );
    }
    if package_too_large {
        if !first_error {
            let _ = errors.push(',');
        }
        let _ = errors.push_str(
            "\"encoding\":{\"code\":\"diag_snapshot_package_too_large\",\"retryable\":true}",
        );
    }
    write_http_chunk(socket, errors.as_str()).await?;
    write_http_chunk(socket, "}}").await?;
    socket_write_all(socket, b"0\r\n\r\n").await
}

fn diag_package_supported(package: &str) -> bool {
    matches!(
        package,
        "core"
            | "mcu.runtime"
            | "bq40.core"
            | "bq40.manufacturing"
            | "bq25792.regs"
            | "tps55288.out_a"
            | "tps55288.out_b"
            | "ina3221.regs"
            | "tmp112.out_a"
            | "tmp112.out_b"
            | "fusb302.regs"
            | "usbpd.policy"
            | "front_panel.io"
            | "derived.power"
    )
}

async fn write_sse_response_head(
    socket: &mut TcpSocket<'_>,
    origin: Option<&str>,
) -> Result<(), embassy_net::tcp::Error> {
    let Some(head) = build_sse_response_head(origin) else {
        return Err(embassy_net::tcp::Error::ConnectionReset);
    };
    socket_write_all(socket, head.as_bytes()).await
}

fn set_wifi_snapshot(snapshot: WifiSnapshot) {
    critical_section::with(|cs| {
        *WIFI_STATE.borrow_ref_mut(cs) = snapshot;
    });
}

fn current_status_snapshot() -> UpsStatusSnapshot {
    critical_section::with(|cs| *UPS_STATUS.borrow_ref(cs))
}

fn current_diag_snapshot() -> DerivedPowerSnapshot {
    critical_section::with(|cs| *DIAG_SNAPSHOT.borrow_ref(cs))
}

fn current_charge_control_detail() -> ChargeControlDetailSnapshot {
    critical_section::with(|cs| *CHARGE_CONTROL_DETAIL.borrow_ref(cs))
}

fn split_request_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    }
}

fn parse_diag_snapshot_query_packages(query: Option<&str>) -> Vec<String<32>, 8> {
    let mut packages = Vec::<String<32>, 8>::new();
    let Some(query) = query else {
        return packages;
    };
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key != "package" || value.is_empty() {
            continue;
        }
        let mut package = String::<32>::new();
        if package.push_str(value).is_ok() {
            let _ = packages.push(package);
        }
    }
    packages
}

fn diag_package_mask(packages: &[String<32>]) -> u32 {
    packages.iter().fold(0, |mask, package| {
        mask | match package.as_str() {
            "bq40.core" => 1 << 0,
            "bq40.manufacturing" => 1 << 1,
            "bq25792.regs" => 1 << 2,
            "tps55288.out_a" => 1 << 3,
            "tps55288.out_b" => 1 << 4,
            "ina3221.regs" => 1 << 5,
            "tmp112.out_a" => 1 << 6,
            "tmp112.out_b" => 1 << 7,
            "fusb302.regs" => 1 << 8,
            _ => 0,
        }
    })
}

async fn request_diag_capture(packages: &[String<32>]) -> Result<(), &'static str> {
    let mask = diag_package_mask(packages);
    if mask == 0 {
        return Ok(());
    }
    if DIAG_CAPTURE_BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("diag_capture_busy");
    }
    let generation = DIAG_CAPTURE_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
        .max(1);
    DIAG_CAPTURE_REQUEST_MASK.store(mask, Ordering::Release);
    DIAG_CAPTURE_REQUEST_GENERATION.store(generation, Ordering::Release);
    let started = embassy_time::Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        if DIAG_CAPTURE_COMPLETE_GENERATION.load(Ordering::Acquire) == generation {
            return Ok(());
        }
        Timer::after(Duration::from_millis(10)).await;
    }
    DIAG_CAPTURE_REQUEST_GENERATION.store(0, Ordering::Release);
    DIAG_CAPTURE_BUSY.store(false, Ordering::Release);
    Err("diag_capture_timeout")
}

fn diag_snapshot_json_complete(body: &str) -> bool {
    body.starts_with(r#"{"schema_version":2,"packages":{"#)
        && body.contains(r#","errors":{"#)
        && body.ends_with("}}")
}

pub fn set_front_panel_runtime(snapshot: FrontPanelRuntimeSnapshot) {
    critical_section::with(|cs| {
        *FRONT_PANEL_RUNTIME.borrow_ref_mut(cs) = snapshot;
    });
}

pub fn current_front_panel_runtime() -> FrontPanelRuntimeSnapshot {
    critical_section::with(|cs| *FRONT_PANEL_RUNTIME.borrow_ref(cs))
}

#[cfg(test)]
pub(crate) const fn status_push_interval_millis_for_test() -> u64 {
    STATUS_PUSH_INTERVAL.as_millis()
}

#[cfg(test)]
mod tests {
    use super::{
        diag_package_mask, diag_snapshot_json_complete, parse_diag_snapshot_query_packages,
        split_request_target,
    };

    #[test]
    fn splits_request_target_query() {
        assert_eq!(
            split_request_target("/api/v1/diag-snapshot?package=bq40.core"),
            ("/api/v1/diag-snapshot", Some("package=bq40.core"))
        );
        assert_eq!(
            split_request_target("/api/v1/status"),
            ("/api/v1/status", None)
        );
    }

    #[test]
    fn parses_repeated_diag_snapshot_package_query() {
        let packages = parse_diag_snapshot_query_packages(Some(
            "include_meta=true&package=bq40.manufacturing&package=bq25792.regs",
        ));
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].as_str(), "bq40.manufacturing");
        assert_eq!(packages[1].as_str(), "bq25792.regs");
    }

    #[test]
    fn diag_snapshot_json_complete_rejects_truncated_payload() {
        assert!(diag_snapshot_json_complete(
            r#"{"schema_version":2,"packages":{},"errors":{}}"#
        ));
        assert!(!diag_snapshot_json_complete(r#"{"packages":{},"errors":{"#));
        assert!(!diag_snapshot_json_complete(r#"{"packages":{}"#));
    }

    #[test]
    fn fresh_hardware_packages_map_to_bridge_bits() {
        let packages = parse_diag_snapshot_query_packages(Some(
            "package=bq40.manufacturing&package=derived.power",
        ));
        assert_eq!(diag_package_mask(packages.as_slice()), 1 << 1);

        let packages = parse_diag_snapshot_query_packages(Some("package=derived.power"));
        assert_eq!(diag_package_mask(packages.as_slice()), 0);
    }
}

fn note_wifi_error(mac: [u8; 6], dns: Option<[u8; 4]>, is_static: bool, error: WifiErrorKind) {
    set_wifi_snapshot(WifiSnapshot {
        state: WifiConnectionState::Error,
        ipv4: None,
        gateway: None,
        dns,
        is_static,
        last_error: Some(error),
        rssi_dbm: None,
        mac: Some(mac),
    });
}

fn build_net_config_from_env() -> (NetConfig, bool, Option<WifiErrorKind>, Option<[u8; 4]>) {
    let parsed = resolve_net_env_config(WIFI_STATIC_IP, WIFI_NETMASK, WIFI_GATEWAY, WIFI_DNS);
    if let Some(static_ipv4) = parsed.static_ipv4 {
        let mut dns_servers = Vec::<Ipv4Address, 3>::new();
        if let Some(dns) = static_ipv4.dns {
            let _ = dns_servers.push(ipv4_from_octets(dns));
        }
        let cfg = StaticConfigV4 {
            address: Ipv4Cidr::new(ipv4_from_octets(static_ipv4.ip), static_ipv4.prefix_len),
            gateway: Some(ipv4_from_octets(static_ipv4.gateway)),
            dns_servers,
        };
        return (
            NetConfig::ipv4_static(cfg),
            true,
            None,
            parsed.configured_dns,
        );
    }

    if parsed.last_error == Some(WifiErrorKind::BadStaticConfig) {
        warn!("net: invalid or incomplete static IPv4 config; fallback to dhcp");
    }

    (
        NetConfig::dhcpv4(DhcpConfig::default()),
        false,
        parsed.last_error,
        parsed.configured_dns,
    )
}

fn ipv4_from_octets(octets: [u8; 4]) -> Ipv4Address {
    Ipv4Address::new(octets[0], octets[1], octets[2], octets[3])
}
