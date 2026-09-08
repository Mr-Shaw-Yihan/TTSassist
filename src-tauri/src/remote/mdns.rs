// mDNS 服务广播：让同一局域网的 App 自动发现本机（_ttsassist-remote._tcp）。
//
// mdns-sd 为纯 Rust 实现（自带线程，Windows 可用）。本机局域网 IP 用
// UDP connect 技巧探测（不真正发包）；探测失败（无网络）跳过广播，
// App 仍可手动填 IP:端口 连接。原 lan-remote 插件 mdns_adv.rs 迁入本体。

use std::net::{IpAddr, ToSocketAddrs, UdpSocket};

/// 是否为可用作局域网地址的私网 IPv4（RFC1918：10/8、172.16/12、192.168/16）
fn is_private_v4(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
        }
        IpAddr::V6(_) => false,
    }
}

/// 探测本机对外局域网 IP：
/// 1) 首选向公网地址 connect 一个 UDP socket（无实际流量），取默认路由出口 IP；
///    仅当它是私网 IPv4 才采信（避免选到环回/链路本地/公网地址）。
/// 2) 纯局域网/无公网路由时回退解析本机主机名，取首个私网 IPv4；
///    避免退化成 127.0.0.1 使面板给出不可达地址。
pub fn local_lan_ip() -> Option<IpAddr> {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local) = sock.local_addr() {
                let ip = local.ip();
                if is_private_v4(&ip) {
                    return Some(ip);
                }
            }
        }
    }
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    if !host.is_empty() {
        if let Ok(addrs) = (host.as_str(), 0u16).to_socket_addrs() {
            for a in addrs {
                if is_private_v4(&a.ip()) {
                    return Some(a.ip());
                }
            }
        }
    }
    None
}

/// 在独立线程启动 mDNS 广播（调用方不等待，失败只记日志）。
/// 本体服务随进程存活，广播随之常驻，无需停播逻辑。
pub fn spawn_broadcast(port: u16) {
    std::thread::Builder::new()
        .name("remote-mdns".into())
        .spawn(move || {
            let Some(ip) = local_lan_ip() else {
                log_warn!("[remote] 未探测到局域网 IP，跳过 mDNS 广播（App 可手动填 IP 连接）");
                return;
            };
            let service_type = "_ttsassist-remote._tcp.local.";
            // 实例名用主机名，多台 PC 同网时可区分
            let instance = hostname();
            match mdns_sd::ServiceDaemon::new() {
                Ok(daemon) => {
                    let host_fqdn = format!("{}.local.", instance);
                    let txt: Option<std::collections::HashMap<String, String>> = None;
                    let info = mdns_sd::ServiceInfo::new(
                        service_type,
                        &instance,
                        &host_fqdn,
                        ip,
                        port,
                        txt,
                    );
                    match info {
                        Ok(info) => match daemon.register(info) {
                            Ok(_receiver) => {
                                log_info!(
                                    "[remote] mDNS 广播已启动: {service_type} {instance} {ip}:{port}"
                                );
                                // daemon 与 receiver 需保持存活——置于线程作用域尾部 drop 前
                                std::thread::park();
                            }
                            Err(e) => {
                                log_error!("[remote] mDNS 注册失败: {e}");
                            }
                        },
                        Err(e) => {
                            log_error!("[remote] mDNS 服务信息构造失败: {e}");
                        }
                    }
                }
                Err(e) => {
                    log_error!("[remote] mDNS 守护进程创建失败: {e}");
                }
            }
        })
        .map(|_| ())
        .map_err(|e| {
            log_error!("[remote] mDNS 广播线程启动失败: {e}");
        })
        .ok();
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "VoiceAssist".to_string())
        .chars()
        .take(32)
        .collect()
}
