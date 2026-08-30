# lan-remote（手机遥控·PC 侧插件）

手机遥控 PC 端：在同一局域网内起一个 WebSocket 服务，配合移动端 App（任务 B，
Flutter）实现「手机发指令、PC 后台执行」。宿主侧零特供逻辑——全部遥控业务都在
本插件内，通过**宿主能力桥**（`requires_host_bridge: true`）反向调用宿主的通用能力
（收藏列表/播放/停止/合成/麦克风开关/播放上一条/状态查询/事件订阅）。

## 安装与运行

```powershell
# 构建 + 打包 + 安装到本机（需先关闭 VoiceAssist）
powershell -ExecutionPolicy Bypass -File .\package.ps1 -Install
```

启动 VoiceAssist 后插件自动加载，WS 服务监听 `0.0.0.0:45271`。

**宿主要求**：`min_app_version = 1.9.0`（宿主能力桥随该版本引入；老宿主会因版本
校验拒载，属预期防护）。

## ⚠ 防火墙授权（必须）

首次启动监听时 Windows 会弹防火墙授权弹窗，**必须点「允许」**，否则手机连不上
（本机 127.0.0.1 不受影响，可先用 test_client.py 本机验证）。若当时点了取消：

```powershell
# 管理员 PowerShell 手动补授权（按实际安装路径调整）
netsh advfirewall firewall add rule name="VoiceAssist lan-remote" dir=in action=allow protocol=TCP localport=45271
```

部分路由器开了「AP 隔离」，手机与 PC 互相不可见——此时 mDNS 发现不到任何设备，
App 内可用「手动填 IP:45271」兜底。

## 发现（mDNS）

- 广播服务类型 `_ttsassist-remote._tcp.local.`，实例名 = 计算机名，端口 45271
- 用 [mdns-sd](https://crates.io/crates/mdns-sd)（纯 Rust，Windows 原生可用，无 C 依赖）
- 本机无局域网 IP（断网/仅环回）时自动跳过广播，只留手动 IP 通道
  （决策记录：mDNS 优先，探测失败降级手动，不再做 UDP 广播兜底）

## 配对

1. 插件启动生成 6 位配对码，经能力桥 `set_own_config` 写入 display 字段，
   在 PC「设置 → 插件服务」面板展示（配对成功/主动刷新后自动更换）；
2. App 输入码 → `pair{code}` → 换回 `token`（32 字节 hex），App 持久化用于重连；
3. token 存于插件数据目录 `data/token.json`，重启宿主后 `hello{token}` 自动恢复；
4. 一对一：新配对/新连接会顶替旧会话（旧连接被服务端关闭、旧 token 作废）；
5. 爆破防护：连续 5 次配对失败进入 30 秒冷却。

## 协议（JSON over ws://host:45271）

完整契约（含字段说明）见 `doc/移动端遥控器设计.md` §三，此处为速查：

- c2s：`pair{code}` / `hello{token}` / `refresh_code` / `list_favorites{ref}` /
  `play_favorite{ref,id}` / `stop{ref}` / `synthesize{ref,text}` / `toggle_mic{ref}` /
  `play_last{ref}` / `ping`
- s2c：`pair_ok{token,state}` / `hello_ok{state}` / `state{state}` /
  `favorites{items}` / `ack{ref,ok,err}` / `event{event}` / `pong` /
  `code_refreshed` / `error{err}`

## 验证客户端

```powershell
pip install websockets
python test_client.py --host 127.0.0.1 --code <PC面板上的6位码>
```

跑通 pair → list_favorites → play_favorite → stop → synthesize → toggle_mic →
play_last 全链路并打印 state/event 推送。已配对过可用 `--token <token>` 走重连路径。

## 卸载

插件页（服务插件分类）卸载，重启宿主生效。卸载即清：
`plugins/lan-remote/` 整目录（含 token）、`plugin_config` 的 display 值、环境变量。
Windows 防火墙授权规则是系统级残留（对其它程序无害），如需清理按上方 netsh 命令
删除对应规则。

## 版本三处同步

改版本需同时改：`Cargo.toml` 的 `version`、`src/lib.rs` 的 `PLUGIN_VERSION`、
`package.ps1` 的 `$Version`。
