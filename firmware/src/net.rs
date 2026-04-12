#![cfg(feature = "net_http")]

use alloc::string::String as AllocString;
use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
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
    wifi::{self, ClientConfig, ModeConfig, WifiController, WifiDevice, WifiEvent},
    Controller as RadioController,
};
use heapless::{String, Vec};
use static_cell::StaticCell;

use crate::{
    mdns::{self, MdnsRuntimeConfig},
    mdns_wire::{derive_device_identity, DeviceIdentity},
    net_contract::{
        accepts_event_stream, is_api_v1_path, render_identity_json, render_network_json,
        render_ping_json, render_status_json, write_error_body, write_sse_event, BuildInfo,
    },
    net_logic::{
        build_http_response_head, build_sse_response_head, origin_reflection_allowed,
        resolve_net_env_config, select_active_dns,
    },
    net_types::{
        NetworkUiSummary, UpsStatusSnapshot, WifiConnectionState, WifiErrorKind, WifiSnapshot,
    },
};

const WIFI_SSID: &str = env!("MAINS_AEGIS_WIFI_SSID");
const WIFI_PSK: &str = env!("MAINS_AEGIS_WIFI_PSK");
const WIFI_HOSTNAME: Option<&str> = option_env!("MAINS_AEGIS_WIFI_HOSTNAME");
const WIFI_STATIC_IP: Option<&str> = option_env!("MAINS_AEGIS_WIFI_STATIC_IP");
const WIFI_NETMASK: Option<&str> = option_env!("MAINS_AEGIS_WIFI_NETMASK");
const WIFI_GATEWAY: Option<&str> = option_env!("MAINS_AEGIS_WIFI_GATEWAY");
const WIFI_DNS: Option<&str> = option_env!("MAINS_AEGIS_WIFI_DNS");

const HTTP_PORT: u16 = 80;
const HTTP_WORKER_COUNT: usize = 3;
const HTTP_RESPONSE_BODY_CAP: usize = 3072;
const SSE_FRAME_CAP: usize = 3328;
const REQUEST_BUF_CAP: usize = 1024;
const STATUS_PUSH_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

static STATUS_SSE_ACTIVE: AtomicBool = AtomicBool::new(false);
static RADIO_CONTROLLER: StaticCell<RadioController<'static>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
static WIFI_STATE: Mutex<RefCell<WifiSnapshot>> =
    Mutex::new(RefCell::new(WifiSnapshot::disabled()));
static UPS_STATUS: Mutex<RefCell<UpsStatusSnapshot>> =
    Mutex::new(RefCell::new(UpsStatusSnapshot::empty()));
static DEVICE_IDENTITY: Mutex<RefCell<Option<DeviceIdentity>>> = Mutex::new(RefCell::new(None));

const BUILD_INFO: BuildInfo = BuildInfo {
    package_version: env!("CARGO_PKG_VERSION"),
    build_profile: env!("FW_BUILD_PROFILE"),
    build_id: env!("FW_BUILD_ID"),
    git_sha: env!("FW_GIT_SHA"),
    src_hash: env!("FW_SRC_HASH"),
    git_dirty: env!("FW_GIT_DIRTY"),
};

pub fn publish_ups_status(snapshot: UpsStatusSnapshot) {
    critical_section::with(|cs| {
        *UPS_STATUS.borrow_ref_mut(cs) = snapshot;
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

pub fn log_wifi_config() {
    info!(
        "net: feature=net_http ssid={} static_ip={:?} hostname_override={:?}",
        WIFI_SSID, WIFI_STATIC_IP, WIFI_HOSTNAME,
    );
}

pub fn spawn_wifi_and_http(spawner: &Spawner, wifi_peripheral: WIFI<'static>) {
    let radio = match radio_init() {
        Ok(radio) => radio,
        Err(err) => {
            warn!("net: radio init failed: {:?}", err);
            return;
        }
    };
    let radio = RADIO_CONTROLLER.init(radio);

    let (controller, interfaces) = match wifi::new(radio, wifi_peripheral, Default::default()) {
        Ok(v) => v,
        Err(err) => {
            warn!("net: wifi::new failed: {:?}", err);
            return;
        }
    };

    let wifi_device: WifiDevice<'static> = interfaces.sta;
    let mac = wifi_device.mac_address();
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
    loop {
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
                .with_ssid(AllocString::from(WIFI_SSID))
                .with_password(AllocString::from(WIFI_PSK)),
        );

        if !matches!(controller.is_started(), Ok(true)) {
            if let Err(err) = controller.set_config(&client_config) {
                warn!("net: set_config failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
                continue;
            }
            if let Err(err) = controller.start_async().await {
                warn!("net: start_async failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
                continue;
            }
        }

        info!("net: connecting to ssid={}", WIFI_SSID);
        match controller.connect_async().await {
            Ok(()) => {
                let mut ready = false;
                for _ in 0..30 {
                    if stack.is_config_up() {
                        ready = true;
                        break;
                    }
                    Timer::after(Duration::from_millis(500)).await;
                }
                if !ready {
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
                    set_wifi_snapshot(WifiSnapshot {
                        state: WifiConnectionState::Connected,
                        ipv4: Some(ip),
                        gateway,
                        dns,
                        is_static,
                        last_error: static_cfg_error,
                        rssi_dbm: None,
                        mac: Some(mac),
                    });
                    backoff_secs = 2;
                    info!(
                        "net: wifi connected ip={}.{}.{}.{}",
                        ip[0], ip[1], ip[2], ip[3]
                    );
                }

                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                warn!("net: wifi disconnected");
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::LinkLost);
                Timer::after(Duration::from_secs(3)).await;
            }
            Err(err) => {
                warn!("net: connect_async failed: {:?}", err);
                note_wifi_error(mac, configured_dns, is_static, WifiErrorKind::ConnectFailed);
                Timer::after(Duration::from_secs(backoff_secs)).await;
                backoff_secs = backoff_secs.saturating_mul(2).min(30);
            }
        }
    }
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

    let mut origin: Option<&str> = None;
    let mut accept_sse = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("origin:") {
            origin = line
                .split_once(':')
                .map(|(_, value)| value.trim())
                .filter(|value| !value.is_empty());
        } else if lower.starts_with("accept:") {
            if let Some((_, value)) = line.split_once(':') {
                accept_sse = accepts_event_stream(value.trim());
            }
        }
    }

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
        "/api/v1/status" if accept_sse => {
            handle_status_sse(socket, origin).await?;
        }
        "/api/v1/status" => {
            render_status_json(&mut body, current_status_snapshot());
            write_http_response(socket, "200 OK", body.as_str(), origin).await?;
        }
        _ => {
            write_error_body(&mut body, "not_found", "not found", false, None);
            write_http_response(socket, "404 Not Found", body.as_str(), origin).await?;
        }
    }

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
