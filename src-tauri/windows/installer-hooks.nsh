; 局域网遥控（lan-remote 插件）所需入站端口的自动放行（安装/卸载钩子）。
; 背景：小白用户对「Windows 防火墙弹窗」和「手动建规则」都不友好，且弹窗
; 默认只放行专用网络——WiFi 被判为公用时放行了也不通（2026-08-30 真机实测）。
; 安装器为 currentUser 模式（非提权），通过 runas 一次性提权执行 netsh
; （整个过程只弹一次 UAC）；用户取消 UAC 时规则不写入，退回防火墙弹窗引导。
; 规则名用 ASCII，避免 NSIS 脚本编码问题；profile=any 覆盖专用/公用网络。

!macro NSIS_HOOK_POSTINSTALL
  ExecShellWait runas "$SYSDIR\cmd.exe" '/c netsh advfirewall firewall delete rule name="VoiceAssist Remote TCP 45271" >nul 2>&1 & netsh advfirewall firewall add rule name="VoiceAssist Remote TCP 45271" dir=in action=allow protocol=TCP localport=45271 profile=any & netsh advfirewall firewall delete rule name="VoiceAssist mDNS UDP 5353" >nul 2>&1 & netsh advfirewall firewall add rule name="VoiceAssist mDNS UDP 5353" dir=in action=allow protocol=UDP localport=5353 profile=any' SW_HIDE
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ExecShellWait runas "$SYSDIR\cmd.exe" '/c netsh advfirewall firewall delete rule name="VoiceAssist Remote TCP 45271" >nul 2>&1 & netsh advfirewall firewall delete rule name="VoiceAssist mDNS UDP 5353" >nul 2>&1' SW_HIDE
!macroend
