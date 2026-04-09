#![cfg(feature = "net_http")]

use defmt::{info, warn};
use embassy_futures::select::{select, Either};
use embassy_net::{
    udp::{PacketMetadata, RecvError, SendError, UdpSocket},
    IpAddress, IpEndpoint, Ipv4Address, Stack,
};
use embassy_time::{Duration, Timer};

use crate::mdns_wire::{
    build_discovery_response, parse_query, query_matches, DeviceIdentity, MdnsQuery,
    MDNS_MULTICAST_V4, MDNS_PORT,
};

const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct MdnsRuntimeConfig {
    pub identity: DeviceIdentity,
    pub port: u16,
}

#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>, cfg: MdnsRuntimeConfig) {
    loop {
        stack.wait_config_up().await;
        let Some(v4) = stack.config_v4() else {
            Timer::after(RETRY_DELAY).await;
            continue;
        };
        let ip = v4.address.address();

        if let Err(err) = stack.join_multicast_group(IpAddress::Ipv4(Ipv4Address::new(
            MDNS_MULTICAST_V4[0],
            MDNS_MULTICAST_V4[1],
            MDNS_MULTICAST_V4[2],
            MDNS_MULTICAST_V4[3],
        ))) {
            warn!("mdns: join multicast failed: {:?}", err);
            Timer::after(RETRY_DELAY).await;
            continue;
        }

        let mut rx_meta = [PacketMetadata::EMPTY; 4];
        let mut tx_meta = [PacketMetadata::EMPTY; 4];
        let mut rx_storage = [0u8; 768];
        let mut tx_storage = [0u8; 768];
        let mut recv_buf = [0u8; 768];
        let mut resp_buf = [0u8; 768];
        let mut socket = UdpSocket::new(
            stack,
            &mut rx_meta,
            &mut rx_storage,
            &mut tx_meta,
            &mut tx_storage,
        );
        socket.set_hop_limit(Some(255));
        if let Err(err) = socket.bind((IpAddress::Ipv4(ip), MDNS_PORT)) {
            warn!("mdns: bind failed: {:?}", err);
            Timer::after(RETRY_DELAY).await;
            continue;
        }

        info!(
            "mdns: announce hostname={} service={}",
            cfg.identity.hostname_fqdn.as_str(),
            cfg.identity.service_instance.as_str()
        );
        let dest = IpEndpoint::new(
            IpAddress::Ipv4(Ipv4Address::new(
                MDNS_MULTICAST_V4[0],
                MDNS_MULTICAST_V4[1],
                MDNS_MULTICAST_V4[2],
                MDNS_MULTICAST_V4[3],
            )),
            MDNS_PORT,
        );
        send_response(&mut socket, &mut resp_buf, &cfg, ip.octets(), dest, None).await;

        let mut announce_timer = Timer::after(ANNOUNCE_INTERVAL);
        loop {
            match select(socket.recv_from(&mut recv_buf), announce_timer).await {
                Either::First(result) => {
                    announce_timer = Timer::after(ANNOUNCE_INTERVAL);
                    match result {
                        Ok((len, meta)) => {
                            if let Some(query) = parse_query(&recv_buf[..len]) {
                                if query_matches(&query, &cfg.identity) {
                                    let dest = if query.unicast_response {
                                        meta.endpoint
                                    } else {
                                        IpEndpoint::new(
                                            IpAddress::Ipv4(Ipv4Address::new(
                                                MDNS_MULTICAST_V4[0],
                                                MDNS_MULTICAST_V4[1],
                                                MDNS_MULTICAST_V4[2],
                                                MDNS_MULTICAST_V4[3],
                                            )),
                                            MDNS_PORT,
                                        )
                                    };
                                    send_response(
                                        &mut socket,
                                        &mut resp_buf,
                                        &cfg,
                                        ip.octets(),
                                        dest,
                                        Some(&query),
                                    )
                                    .await;
                                }
                            }
                        }
                        Err(RecvError::Truncated) => warn!("mdns: truncated query"),
                    }
                }
                Either::Second(_) => {
                    send_response(&mut socket, &mut resp_buf, &cfg, ip.octets(), dest, None).await;
                    announce_timer = Timer::after(ANNOUNCE_INTERVAL);
                }
            }

            if !stack.is_config_up() {
                break;
            }
        }
    }
}

async fn send_response(
    socket: &mut UdpSocket<'_>,
    buf: &mut [u8],
    cfg: &MdnsRuntimeConfig,
    ip: [u8; 4],
    dest: IpEndpoint,
    query: Option<&MdnsQuery>,
) {
    let Some(len) = build_discovery_response(buf, &cfg.identity, ip, cfg.port, query) else {
        warn!("mdns: encode response failed");
        return;
    };

    if let Err(err) = socket.send_to(&buf[..len], dest).await {
        match err {
            SendError::NoRoute => warn!("mdns: send no route"),
            SendError::SocketNotBound => warn!("mdns: socket not bound"),
            SendError::PacketTooLarge => warn!("mdns: packet too large"),
        }
    }
}
