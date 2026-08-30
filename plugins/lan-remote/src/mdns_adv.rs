// mDNS 服务广播：让同一局域网的 App 自动发现本机（_ttsassist-remote._tcp）。
//
// mdns-sd 为纯 Rust 实现（自带线程，Windows 可用）。本机局域网 IP 用
// UDP connect 技巧探测（不真正发包）；探测失败（无网络）跳过广播，
// App 仍可手动填 IP:端口 连接。

use std::net::{IpAddr, UdpSocket};

/// 探测本机对外局域网 IP：向公网地址 connect 一个 UDP socket（无实际流量），
// 读 local_addr 即为本机出口 IP。
pub fn local_lan_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip())
}

/// 在独立线程启动 mDNS 广播（attach 线程不等待，失败只记日志）。
/// dll 常驻进程，广播随进程存活，无需停播逻辑。
pub fn spawn_broadcast(port: u16) {
    std::thread::Builder::new()
        .name("lan-remote-mdns".into())
        .spawn(move || {
            let Some(ip) = local_lan_ip() else {
                eprintln!("[lan-remote] 未探测到局域网 IP，跳过 mDNS 广播（App 可手动填 IP 连接）");
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
                                eprintln!(
                                    "[lan-remote] mDNS 广播已启动: {service_type} {instance} {ip}:{port}"
                                );
                                // daemon 与 receiver 需保持存活——置于线程作用域尾部 drop 前
                                std::thread::park();
                            }
                            Err(e) => {
                                eprintln!("[lan-remote] mDNS 注册失败: {e}");
                            }
                        },
                        Err(e) => {
                            eprintln!("[lan-remote] mDNS 服务信息构造失败: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[lan-remote] mDNS 守护进程创建失败: {e}");
                }
            }
        })
        .map(|_| ())
        .map_err(|e| {
            eprintln!("[lan-remote] mDNS 广播线程启动失败: {e}");
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
