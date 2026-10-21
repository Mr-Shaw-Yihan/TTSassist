; TTSassist 安装/卸载钩子。
;
; 安装：局域网遥控（lan-remote 插件）所需入站端口的自动放行。
; 背景：小白用户对「Windows 防火墙弹窗」和「手动建规则」都不友好，且弹窗
; 默认只放行专用网络——WiFi 被判为公用时放行了也不通（2026-08-30 真机实测）。
; 安装器为 currentUser 模式（非提权），通过 runas 一次性提权执行 netsh
; （整个过程只弹一次 UAC）；用户取消 UAC 时规则不写入，退回防火墙弹窗引导。
; 规则名用 ASCII，避免 NSIS 脚本编码问题；profile=any 覆盖专用/公用网络。
;
; 卸载：自定义一页两个勾选框（默认都不勾），分别删除「配置」与「我的数据」。
;   - 「我的数据」的真实位置写在 storage.json 指针里（可能在其他盘），用 PowerShell
;     解析后写成 UTF-16 临时文件再读回，避免 NSIS 直接读 UTF-8 中文路径乱码。
;   - 只删固定清单（消息/收藏/字幕历史/音频/样本）与迁移备份目录，绝不整根递归删除，
;     删除目标先写入 $INSTDIR\uninstall-cleanup.log 便于事后追查。
;   - Tauri 内置的「删除应用数据」勾选框会递归删整个配置根，语义过粗；本钩子把它
;     视为「同时勾选两项」，并在清理前清零其状态，改用上面的精确清单。
;   - 更新（/UPDATE）、静默与被动模式下不弹本页、不删任何数据。

!include "LogicLib.nsh"
!include "nsDialogs.nsh"
!include "FileFunc.nsh"

!define VA_CONFIG_ROOT "$APPDATA\com.voiceassist.app"

Var VaDeleteConfigCheckbox
Var VaDeleteDataCheckbox
Var VaDeleteConfigState
Var VaDeleteDataState
Var VaContentRoot
Var VaCleanupLog
Var VaLogHandle

; 自定义卸载页放在确认页之前（hooks 在 Tauri 的页面声明之前被 include）。
UninstPage custom un.VaCleanupPage un.VaCleanupPageLeave

Function un.VaCleanupPage
  ; 更新/被动安装时跳过本页，避免升级过程中误删数据。
  ${GetOptions} $CMDLINE "/UPDATE" $R0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}
  ${GetOptions} $CMDLINE "/P" $R0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}

  Call un.VaResolveContentRoot
  StrCpy $VaContentRoot $0

  !insertmacro MUI_HEADER_TEXT "卸载 TTSassist" "选择是否一并删除本机数据（默认保留）"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 24u "卸载后默认保留你的「配置」与「我的数据」，重新安装即可继续使用。$\r$\n如需彻底清除，请勾选下面的选项："
  Pop $0

  ${NSD_CreateCheckBox} 0 30u 100% 12u "删除配置（设置、快捷键、API Key、音色、数据位置指针）"
  Pop $VaDeleteConfigCheckbox

  ${NSD_CreateCheckBox} 0 46u 100% 12u "删除我的数据（消息记录、合成音频、收藏夹、克隆样本）"
  Pop $VaDeleteDataCheckbox

  ${If} $VaContentRoot == "${VA_CONFIG_ROOT}"
    ${NSD_CreateLabel} 0 62u 100% 18u "「我的数据」当前位置：$VaContentRoot（默认位置）"
    Pop $0
  ${Else}
    ${NSD_CreateLabel} 0 62u 100% 30u "「我的数据」当前位置：$VaContentRoot$\r$\n注意：它不在默认位置，只勾选「删除配置」不会删除这些数据，需要同时勾选第二项。"
    Pop $0
  ${EndIf}

  ${NSD_CreateLabel} 0 100u 100% 32u "默认都不勾选：卸载后重装仍可找回。$\r$\n下一页的「删除应用数据」等同同时勾选以上两项。"
  Pop $0

  nsDialogs::Show
FunctionEnd

Function un.VaCleanupPageLeave
  ${NSD_GetState} $VaDeleteConfigCheckbox $VaDeleteConfigState
  ${NSD_GetState} $VaDeleteDataCheckbox $VaDeleteDataState
FunctionEnd

; 解析「我的数据」位置：storage.json 指针 -> 配置根（解析失败时的安全回退）。
; 输出 $0；结果同时留在 $VaContentRoot 之外的寄存器里由调用方取走。
Function un.VaResolveContentRoot
  StrCpy $0 "${VA_CONFIG_ROOT}"
  IfFileExists "${VA_CONFIG_ROOT}\storage.json" 0 va_resolve_done

  Delete "${VA_CONFIG_ROOT}\_uninstall-content-root.txt"
  nsExec::ExecToStack `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$ErrorActionPreference='Stop'; $$j=Get-Content -LiteralPath (Join-Path $$env:APPDATA 'com.voiceassist.app\storage.json') -Raw -Encoding UTF8 | ConvertFrom-Json; if ($$j.content_root) { [IO.File]::WriteAllText((Join-Path $$env:APPDATA 'com.voiceassist.app\_uninstall-content-root.txt'), [string]$$j.content_root, [Text.Encoding]::Unicode) }"`
  Pop $1
  Pop $2

  IfFileExists "${VA_CONFIG_ROOT}\_uninstall-content-root.txt" 0 va_resolve_done
    ClearErrors
    FileOpen $1 "${VA_CONFIG_ROOT}\_uninstall-content-root.txt" r
    IfErrors va_resolve_done
    FileReadUTF16LE $1 $2
    FileClose $1
    ; 去掉可能的行尾（WriteAllText 通常不带，这里防御一手）
    StrCpy $3 $2 1 -1
    ${If} $3 == "$\r"
    ${OrIf} $3 == "$\n"
      StrCpy $2 $2 -1
    ${EndIf}
    StrCpy $3 $2 1 -1
    ${If} $3 == "$\r"
    ${OrIf} $3 == "$\n"
      StrCpy $2 $2 -1
    ${EndIf}
    ${If} $2 != ""
      StrCpy $0 $2
    ${EndIf}

  va_resolve_done:
  Delete "${VA_CONFIG_ROOT}\_uninstall-content-root.txt"
FunctionEnd

; 写一行清理日志（$R9 = 内容）。句柄由 un.VaInitCleanupLog 保持打开：
; 每次重开文件再写 UTF-16 会覆盖文件头，必须一次打开、连续写入、最后关闭。
!macro VA_LOG_LINE
  ${If} $VaLogHandle != ""
    FileWriteUTF16LE $VaLogHandle "$R9$\r$\n"
  ${EndIf}
!macroend

Function un.VaInitCleanupLog
  StrCpy $VaCleanupLog "$INSTDIR\uninstall-cleanup.log"
  StrCpy $VaLogHandle ""
  ClearErrors
  FileOpen $VaLogHandle "$VaCleanupLog" w
  ${If} ${Errors}
    StrCpy $VaLogHandle ""
    Return
  ${EndIf}
  FileWriteUTF16LE /BOM $VaLogHandle "TTSassist 卸载清理记录$\r$\n"
  FileWriteUTF16LE $VaLogHandle "配置根：${VA_CONFIG_ROOT}$\r$\n"
  FileWriteUTF16LE $VaLogHandle "我的数据：$VaContentRoot$\r$\n"
  FileWriteUTF16LE $VaLogHandle "删除配置：$VaDeleteConfigState  删除我的数据：$VaDeleteDataState（1=勾选）$\r$\n"
FunctionEnd

Function un.VaDeleteConfig
  ${If} $VaDeleteConfigState != ${BST_CHECKED}
    Return
  ${EndIf}

  StrCpy $R9 "== 删除配置 =="
  !insertmacro VA_LOG_LINE

  StrCpy $R9 "删除文件：${VA_CONFIG_ROOT}\settings.json"
  !insertmacro VA_LOG_LINE
  Delete "${VA_CONFIG_ROOT}\settings.json"

  StrCpy $R9 "删除文件：${VA_CONFIG_ROOT}\remote-pairing.json"
  !insertmacro VA_LOG_LINE
  Delete "${VA_CONFIG_ROOT}\remote-pairing.json"

  StrCpy $R9 "删除文件：${VA_CONFIG_ROOT}\remote-config-cache.json"
  !insertmacro VA_LOG_LINE
  Delete "${VA_CONFIG_ROOT}\remote-config-cache.json"

  StrCpy $R9 "删除文件：${VA_CONFIG_ROOT}\storage.json"
  !insertmacro VA_LOG_LINE
  Delete "${VA_CONFIG_ROOT}\storage.json"

  StrCpy $R9 "删除目录：${VA_CONFIG_ROOT}\logs"
  !insertmacro VA_LOG_LINE
  RMDir /r "${VA_CONFIG_ROOT}\logs"
FunctionEnd

Function un.VaDeleteData
  ${If} $VaDeleteDataState != ${BST_CHECKED}
    Return
  ${EndIf}

  StrCpy $R9 "== 删除「我的数据」：$VaContentRoot =="
  !insertmacro VA_LOG_LINE

  ; 安全校验：必须是 "X:\..." 形式的本地绝对路径，且长度足够（避免误删盘根）。
  StrLen $R0 $VaContentRoot
  ${If} $R0 < 4
    StrCpy $R9 "跳过：位置异常（过短）"
    !insertmacro VA_LOG_LINE
    Return
  ${EndIf}
  StrCpy $R1 $VaContentRoot 1 1
  StrCpy $R2 $VaContentRoot 1 2
  ${If} $R1 != ":"
  ${OrIf} $R2 != "\"
    StrCpy $R9 "跳过：不是本地绝对路径"
    !insertmacro VA_LOG_LINE
    Return
  ${EndIf}

  ; 只有目录确实像「我的数据」时才动手，防止指针被改坏后误删别的目录。
  ${IfNot} ${FileExists} "$VaContentRoot\messages.json"
  ${AndIfNot} ${FileExists} "$VaContentRoot\favorites.json"
  ${AndIfNot} ${FileExists} "$VaContentRoot\subtitle_history.json"
  ${AndIfNot} ${FileExists} "$VaContentRoot\audio\*.*"
  ${AndIfNot} ${FileExists} "$VaContentRoot\voice_samples\*.*"
    StrCpy $R9 "跳过：目录中没有检测到「我的数据」特征文件"
    !insertmacro VA_LOG_LINE
    Return
  ${EndIf}

  StrCpy $R9 "删除文件：$VaContentRoot\messages.json"
  !insertmacro VA_LOG_LINE
  Delete "$VaContentRoot\messages.json"

  StrCpy $R9 "删除文件：$VaContentRoot\favorites.json"
  !insertmacro VA_LOG_LINE
  Delete "$VaContentRoot\favorites.json"

  StrCpy $R9 "删除文件：$VaContentRoot\subtitle_history.json"
  !insertmacro VA_LOG_LINE
  Delete "$VaContentRoot\subtitle_history.json"

  StrCpy $R9 "删除目录：$VaContentRoot\audio"
  !insertmacro VA_LOG_LINE
  RMDir /r "$VaContentRoot\audio"

  StrCpy $R9 "删除目录：$VaContentRoot\voice_samples"
  !insertmacro VA_LOG_LINE
  RMDir /r "$VaContentRoot\voice_samples"

  Call un.VaDeleteBackups
FunctionEnd

; 迁移会留下 .migrated-backup-*（配置根）与 backup-*（数据根）的完整副本。
; 用户勾选「删除我的数据」时一并清掉，否则消息与音频仍留在备份里。
Function un.VaDeleteBackups
  FindFirst $R0 $R1 "${VA_CONFIG_ROOT}\.migrated-backup-*"
  va_bak_cfg_loop:
    StrCmp $R1 "" va_bak_cfg_done
    StrCpy $R9 "删除备份：${VA_CONFIG_ROOT}\$R1"
    !insertmacro VA_LOG_LINE
    RMDir /r "${VA_CONFIG_ROOT}\$R1"
    FindNext $R0 $R1
    Goto va_bak_cfg_loop
  va_bak_cfg_done:
  FindClose $R0

  FindFirst $R0 $R1 "$VaContentRoot\backup-*"
  va_bak_data_loop:
    StrCmp $R1 "" va_bak_data_done
    StrCpy $R9 "删除备份：$VaContentRoot\$R1"
    !insertmacro VA_LOG_LINE
    RMDir /r "$VaContentRoot\$R1"
    FindNext $R0 $R1
    Goto va_bak_data_loop
  va_bak_data_done:
  FindClose $R0
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  ExecShellWait runas "$SYSDIR\cmd.exe" '/c netsh advfirewall firewall delete rule name="VoiceAssist Remote TCP 45271" >nul 2>&1 & netsh advfirewall firewall add rule name="VoiceAssist Remote TCP 45271" dir=in action=allow protocol=TCP localport=45271 profile=any & netsh advfirewall firewall delete rule name="VoiceAssist mDNS UDP 5353" >nul 2>&1 & netsh advfirewall firewall add rule name="VoiceAssist mDNS UDP 5353" dir=in action=allow protocol=UDP localport=5353 profile=any' SW_HIDE
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 局域网遥控的防火墙规则（原有行为）
  ExecShellWait runas "$SYSDIR\cmd.exe" '/c netsh advfirewall firewall delete rule name="VoiceAssist Remote TCP 45271" >nul 2>&1 & netsh advfirewall firewall delete rule name="VoiceAssist mDNS UDP 5353" >nul 2>&1' SW_HIDE

  ; 更新模式不删数据：升级必须先保住用户的消息与音频。
  ${If} $UpdateMode <> 1
    ; 内置「删除应用数据」= 同时勾选两项；随后清零，避免模板再递归删整个配置根。
    ${If} $DeleteAppDataCheckboxState = ${BST_CHECKED}
      StrCpy $VaDeleteConfigState ${BST_CHECKED}
      StrCpy $VaDeleteDataState ${BST_CHECKED}
    ${EndIf}
    StrCpy $DeleteAppDataCheckboxState 0

    Call un.VaResolveContentRoot
    StrCpy $VaContentRoot $0
    Call un.VaInitCleanupLog
    Call un.VaDeleteConfig
    Call un.VaDeleteData
    ${If} $VaLogHandle != ""
      FileClose $VaLogHandle
      StrCpy $VaLogHandle ""
    ${EndIf}
  ${EndIf}
!macroend
