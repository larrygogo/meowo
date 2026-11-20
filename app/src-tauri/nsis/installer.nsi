Unicode true
ManifestDPIAware true
; Add in `dpiAwareness` `PerMonitorV2` to manifest for Windows 10 1607+ (note this should not affect lower versions since they should be able to ignore this and pick up `dpiAware` `true` set by `ManifestDPIAware true`)
; Currently undocumented on NSIS's website but is in the Docs folder of source tree, see
; https://github.com/kichik/nsis/blob/5fc0b87b819a9eec006df4967d08e522ddd651c9/Docs/src/attributes.but#L286-L300
; https://github.com/tauri-apps/tauri/pull/10106
ManifestDPIAwareness PerMonitorV2

!if "{{compression}}" == "none"
  SetCompress off
!else
  ; Set the compression algorithm. We default to LZMA.
  SetCompressor /SOLID "{{compression}}"
!endif

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include "utils.nsh"
!include "FileAssociation.nsh"
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "StrFunc.nsh"
${StrCase}
${StrLoc}

{{#if installer_hooks}}
!include "{{installer_hooks}}"
{{/if}}

!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define HOMEPAGE "{{homepage}}"
!define INSTALLMODE "{{install_mode}}"
!define LICENSE "{{license}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define UNINSTALLERICON "{{uninstaller_icon}}"
!define UNINSTALLERHEADERIMAGE "{{uninstaller_header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define BUNDLEID "{{bundle_id}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define ARCH "{{arch}}"
!define ADDITIONALPLUGINSPATH "{{additional_plugins_path}}"
!define ALLOWDOWNGRADES "{{allow_downgrades}}"
!define DISPLAYLANGUAGESELECTOR "{{display_language_selector}}"
!define INSTALLWEBVIEW2MODE "{{install_webview2_mode}}"
!define WEBVIEW2INSTALLERARGS "{{webview2_installer_args}}"
!define WEBVIEW2BOOTSTRAPPERPATH "{{webview2_bootstrapper_path}}"
!define WEBVIEW2INSTALLERPATH "{{webview2_installer_path}}"
!define MINIMUMWEBVIEW2VERSION "{{minimum_webview2_version}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUKEY "Software\${MANUFACTURER}"
!define MANUPRODUCTKEY "${MANUKEY}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"
!define STARTMENUFOLDER "{{start_menu_folder}}"

Var PassiveMode
Var UpdateMode
Var NoShortcutMode
Var WixMode
Var OldMainBinaryName

Name "${PRODUCTNAME}"
BrandingText "${COPYRIGHT}"
OutFile "${OUTFILE}"

; We don't actually use this value as default install path,
; it's just for nsis to append the product name folder in the directory selector
; https://nsis.sourceforge.io/Reference/InstallDir
!define PLACEHOLDER_INSTALL_DIR "placeholder\${PRODUCTNAME}"
InstallDir "${PLACEHOLDER_INSTALL_DIR}"

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

# additional plugins
!addplugindir "${ADDITIONALPLUGINSPATH}"

; Uninstaller signing command
!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

; Handle install mode, `perUser`, `perMachine` or `both`
!if "${INSTALLMODE}" == "perMachine"
  RequestExecutionLevel admin
!endif

!if "${INSTALLMODE}" == "currentUser"
  RequestExecutionLevel user
!endif

!if "${INSTALLMODE}" == "both"
  !define MULTIUSER_MUI
  !define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
  !define MULTIUSER_INSTALLMODE_COMMANDLINE
  !if "${ARCH}" == "x64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !else if "${ARCH}" == "arm64"
    !define MULTIUSER_USE_PROGRAMFILES64
  !endif
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
  !define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
  !define MULTIUSER_INSTALLMODEPAGE_SHOWUSERNAME
  !define MULTIUSER_INSTALLMODE_FUNCTION RestorePreviousInstallLocation
  !define MULTIUSER_EXECUTIONLEVEL Highest
  !include MultiUser.nsh
!endif

; Installer icon
!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
!endif

; Installer sidebar image
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif

; Enable header images for installer and uninstaller pages when either image is configured.
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!else if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
!endif

; Installer header image
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
!endif

; Uninstaller header image
!if "${UNINSTALLERHEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE_UNBITMAP "${UNINSTALLERHEADERIMAGE}"
!endif

; Uninstaller icon
!if "${UNINSTALLERICON}" != ""
  !define MUI_UNICON "${UNINSTALLERICON}"
!endif

; Define registry key to store installer language
!define MUI_LANGDLL_REGISTRY_ROOT "HKCU"
!define MUI_LANGDLL_REGISTRY_KEY "${MANUPRODUCTKEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "Installer Language"

; Installer pages, must be ordered as they appear
; MEOWO-BEGIN oneclick-ui
; 一键安装自绘界面：上游的 WELCOME / 重装问询 / DIRECTORY / STARTMENU / FINISH 五页
; 替换为「自绘主页 → INSTFILES(SHOW 回调改造) → 自绘完成页」。卸载侧 MUI 页原样保留。
; 新增状态一律用 Meowo* 命名 Var，不碰 $R0-$R9（$R0-$R3 归 utils.nsh 的
; CheckIfAppIsRunning，$R4-$R9 归 nsis-hooks.nsh；重装检测沿用上游 $R0/$R6/$WixMode
; 约定——$R0 的版本比较结果从页面创建活到 leave，期间任何新代码不得写 $R0/$R6）。
;
; 色值与 icons/nsis-oneclick-bg.bmp 的分区底色一一对应（scripts/generate-nsis-bitmaps.py
; 有同名注释）：原生控件文字背景只能靠 SetCtlColors 配纯色，控件落在哪个分区就配哪个色。
!define MEOWO_COL_BASE   "17171a"
!define MEOWO_COL_CARD   "1d1d21"
!define MEOWO_COL_FIELD  "141416"
!define MEOWO_COL_TEXT   "f5f5f7"
!define MEOWO_COL_SUB    "a0a0a8"
!define MEOWO_COL_FAINT  "6e6e76"
!define MEOWO_COL_LINK   "d4f64c"
!define MEOWO_COL_ONLINK "1c2600"
; 布局（逻辑 px，96dpi）。与 scripts/generate-nsis-bitmaps.py 的同名几何常量一一对应：
; 底图上烙好的胶囊、卡片、输入框外框、勾选框外框都按这组坐标画，控件按同一组坐标叠上去。
!define MEOWO_W      640
!define MEOWO_H      400
!define MEOWO_EXPAND 92
!define /ifndef SS_CENTER 0x1
!define /ifndef SS_RIGHT 0x2
!define /ifndef SS_CENTERIMAGE 0x200
!define /ifndef SS_NOTIFY 0x100
!define /ifndef SS_BITMAP 0xE
!define /ifndef ES_AUTOHSCROLL 0x80
!define /ifndef SW_HIDE 0
!define /ifndef SW_SHOW 5
; 背景位图与 headerImage 同目录：从 HEADERIMAGE（bundler 渲染时已绝对化）推导，
; 不依赖 makensis 的 CWD。重命名这些文件时必须同步这里。
; 三个变体：无后缀给进度页（没有按钮），-btn 给主页收起态与完成页，-expanded 给主页展开态。
!searchreplace MEOWO_BG_PATH "${HEADERIMAGE}" "nsis-header.bmp" "nsis-oneclick-bg.bmp"
!searchreplace MEOWO_BGBTN_PATH "${HEADERIMAGE}" "nsis-header.bmp" "nsis-oneclick-bg-btn.bmp"
!searchreplace MEOWO_BGEXP_PATH "${HEADERIMAGE}" "nsis-header.bmp" "nsis-oneclick-bg-expanded.bmp"
ReserveFile "${MEOWO_BG_PATH}"
ReserveFile "${MEOWO_BGBTN_PATH}"
ReserveFile "${MEOWO_BGEXP_PATH}"
; 勾选框两态按显示缩放各一套（icons/nsis-ui/toggle-{on,off}-{dpi}.bmp），按 DPI 取原尺寸那张，不拉伸。
!searchreplace MEOWO_UI_DIR "${HEADERIMAGE}" "nsis-header.bmp" "nsis-ui"

Var MeowoDpi
Var MeowoW            ; 外窗 client 宽/高（物理 px，MeowoFitClient 填）
Var MeowoH
Var MeowoFrameW       ; 外框尺寸 - client 尺寸（边框 + 标题栏），改窗口高度时用
Var MeowoFrameH
Var MeowoTitleFont
Var MeowoBodyFont
Var MeowoBtnFont
Var MeowoSmallFont
Var MeowoIconFont
Var MeowoWordFont
Var MeowoUiDpi        ; 勾选框位图取哪一档（96/120/144/168/192/240/288）
Var MeowoToggleOn     ; 勾选框两态的 HBITMAP（主页建页时加载，整页复用）
Var MeowoToggleOff
Var MeowoHasExisting  ; 0/1：检测到已有安装（MeowoDetectExisting 填）
Var MeowoDesktopLnk   ; 1/0：「创建桌面快捷方式」勾选
Var MeowoExpanded     ; 0/1：自定义安装选项是否展开
Var MeowoPage         ; 当前自绘页（nsDialogs 对话框）
Var MeowoBgCtl        ; 当前自绘页的背景位图控件
Var MeowoBgHandle     ; 背景 HBITMAP（NSD_SetStretchedImage 管理）
Var MeowoDirLabel
Var MeowoDirText
Var MeowoChangeLink
Var MeowoDesktopBox
Var MeowoDesktopLabel
Var MeowoCustomLink
Var MeowoNoteLabel
Var MeowoVersionLabel

; 逻辑像素 → 物理像素（96dpi 基准，MeowoGuiInit 之后可用）。
!macro MeowoScale out in
  IntOp ${out} ${in} * $MeowoDpi
  IntOp ${out} ${out} / 96
!macroend
!define MeowoScale "!insertmacro MeowoScale"

; 在当前 nsDialogs 页上按逻辑坐标建一个文字控件（STATIC）。占用 $2-$5，不碰 $R*。
!macro MeowoText out x y w h style text font fg bg
  ${MeowoScale} $2 ${x}
  ${MeowoScale} $3 ${y}
  ${MeowoScale} $4 ${w}
  ${MeowoScale} $5 ${h}
  nsDialogs::CreateControl STATIC "${__NSD_Label_STYLE}|${style}" "${__NSD_Label_EXSTYLE}" $2 $3 $4 $5 "${text}"
  Pop ${out}
  SetCtlColors ${out} "${fg}" "${bg}"
  SendMessage ${out} ${WM_SETFONT} ${font} 1
!macroend
!define MeowoText "!insertmacro MeowoText"

; 自绘勾选：一个 SS_BITMAP 静态控件，按状态换成预先画好的圆角位图（选中 = 青柠圆角块 + 对勾）。
; 不用原生复选框：themed 复选框的框是系统蓝、文字不吃 SetCtlColors（实拍黑字糊在暗底上）；
; 也不用「圆角框里叠一块变色方块」：选中时那块是直角（leader：「勾选了之后没有圆角」）。
!macro MeowoPaintToggle ctl state
  ${If} ${state} = 1
    SendMessage ${ctl} 0x172 0 $MeowoToggleOn   ; STM_SETIMAGE(IMAGE_BITMAP)
  ${Else}
    SendMessage ${ctl} 0x172 0 $MeowoToggleOff
  ${EndIf}
!macroend
!define MeowoPaintToggle "!insertmacro MeowoPaintToggle"

; 一次性改造向导外框：去掉最大化、resize 到设计尺寸并居中、隐藏向导 chrome、内页占位控件(1018)撑满。
; 之后所有页面（含 MUI 的 INSTFILES）都按全屏 client 区排版。passive 也走这里
; （/P 进度窗与 GUI 同一视觉——但 updater 已改传 /S 全静默，见 README「必须保留的
; 语义」）；silent 无窗口直接返回。
!define MUI_CUSTOMFUNCTION_GUIINIT MeowoGuiInit
Function MeowoGuiInit
  ${If} ${Silent}
    Return
  ${EndIf}
  ; 输出走寄存器再转存：System::Call 对自定义 Var 的输出说明符支持不可靠。
  System::Call "user32::GetDpiForWindow(p $HWNDPARENT) i .r0"
  StrCpy $MeowoDpi $0
  ${If} $MeowoDpi < 96
    StrCpy $MeowoDpi 96
  ${EndIf}
  ; 勾选框位图档位：取最接近的一档（常见缩放 100/125/150/175/200/250/300% 正好命中）。
  ${If} $MeowoDpi >= 264
    StrCpy $MeowoUiDpi 288
  ${ElseIf} $MeowoDpi >= 216
    StrCpy $MeowoUiDpi 240
  ${ElseIf} $MeowoDpi >= 180
    StrCpy $MeowoUiDpi 192
  ${ElseIf} $MeowoDpi >= 156
    StrCpy $MeowoUiDpi 168
  ${ElseIf} $MeowoDpi >= 132
    StrCpy $MeowoUiDpi 144
  ${ElseIf} $MeowoDpi >= 108
    StrCpy $MeowoUiDpi 120
  ${Else}
    StrCpy $MeowoUiDpi 96
  ${EndIf}

  ; 去掉最大化按钮（WS_MAXIMIZEBOX = 0x10000）：固定尺寸的安装窗口不该能最大化。
  System::Call "user32::GetWindowLongW(p $HWNDPARENT, i -16) i .r0"
  IntOp $0 $0 & 0xFFFEFFFF
  System::Call "user32::SetWindowLongW(p $HWNDPARENT, i -16, i r0)"

  ; 量出边框差（外框 - client），按设计 client 尺寸反推外框，以**原窗口中心**为锚重定位：
  ; NSIS 已把窗口按启动显示器居中，拿主屏 GetSystemMetrics 重算会在多屏环境跑偏（实测跑到角落）。
  System::Call "*(i 0, i 0, i 0, i 0) p .r4"
  System::Call "user32::GetWindowRect(p $HWNDPARENT, p r4)"
  System::Call "*$4(i .r2, i .r3, i .r5, i .r6)"
  System::Call "user32::GetClientRect(p $HWNDPARENT, p r4)"
  System::Call "*$4(i, i, i .r7, i .r8)"
  System::Free $4
  IntOp $0 $5 - $2
  IntOp $MeowoFrameW $0 - $7
  IntOp $0 $6 - $3
  IntOp $MeowoFrameH $0 - $8
  IntOp $2 $2 + $5
  IntOp $2 $2 / 2         ; 原中心 x
  IntOp $3 $3 + $6
  IntOp $3 $3 / 2         ; 原中心 y
  ${MeowoScale} $0 ${MEOWO_W}
  IntOp $0 $0 + $MeowoFrameW
  ${MeowoScale} $1 ${MEOWO_H}
  IntOp $1 $1 + $MeowoFrameH
  IntOp $5 $0 / 2
  IntOp $2 $2 - $5
  IntOp $5 $1 / 2
  IntOp $3 $3 - $5
  ; SWP_NOZORDER|SWP_NOACTIVATE|SWP_FRAMECHANGED = 0x34（FRAMECHANGED 让去掉的最大化按钮立即生效）。
  System::Call "user32::SetWindowPos(p $HWNDPARENT, p 0, i r2, i r3, i r0, i r1, i 0x34)"

  ; 隐藏向导 chrome。「下一步」(ID 1) 只是视觉隐藏——自绘按钮靠
  ; SendMessage WM_COMMAND 1 驱动它走标准页流（leave 校验照常触发）。
  ; 1028/1256=branding，1034-1038=header 区，1045=按钮上方分隔线。
  Call MeowoHideChrome
  StrCpy $MeowoPage ""
  StrCpy $MeowoBgCtl ""
  Call MeowoFitClient

  ; 标题栏切暗色，与深色内容匹配（DWMWA_USE_IMMERSIVE_DARK_MODE=20；Win10 老构建
  ; 不认该属性，静默失败退回亮色）。
  System::Call "dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i 1, i 4)"
  ; 隐藏 chrome + 改窗口尺寸后强制整窗重绘：被隐藏控件与新暴露区域不会自动擦除，
  ; 不刷会在右/下边缘留未初始化的花屏残影（实拍）。
  System::Call "user32::InvalidateRect(p $HWNDPARENT, p 0, i 1)"

  ; 字体（逻辑 px 随 DPI 缩放；DEFAULT_CHARSET=1，CLEARTYPE_QUALITY=5）。显式用微软雅黑 UI——
  ; $(^Font) 是 MS Shell Dlg，中文会映射到宋体，衬线在暗色大字下惨不忍睹（实拍）。
  ; 标题 20 / 正文 14 / 按钮 15 / 小字 12 / 图标 16（字标 30 见末尾）。
  ${MeowoScale} $0 20
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 600, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Microsoft YaHei UI') p .s"
  Pop $MeowoTitleFont
  ${MeowoScale} $0 14
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 400, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Microsoft YaHei UI') p .s"
  Pop $MeowoBodyFont
  ${MeowoScale} $0 15
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 600, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Microsoft YaHei UI') p .s"
  Pop $MeowoBtnFont
  ${MeowoScale} $0 12
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 400, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Microsoft YaHei UI') p .s"
  Pop $MeowoSmallFont
  ; 图标字体：Windows 10/11 自带的 Segoe MDL2 Assets（矢量字形，任何缩放都清晰，能配色）。
  ${MeowoScale} $0 16
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 400, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Segoe MDL2 Assets') p .s"
  Pop $MeowoIconFont
  ; 字标「Meowo」：Segoe UI Bold 30（拉丁字标，与应用图标旁的字标同字族）。
  ${MeowoScale} $0 30
  IntOp $0 0 - $0
  System::Call "gdi32::CreateFont(i r0, i 0, i 0, i 0, i 700, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t 'Segoe UI') p .s"
  Pop $MeowoWordFont
FunctionEnd

!macro MeowoHideDlgItem id
  GetDlgItem $0 $HWNDPARENT ${id}
  ShowWindow $0 ${SW_HIDE}
!macroend

; 隐藏向导自带的导航按钮与 header/branding。NSIS 换页时会按页面能力重新显示导航按钮
; （实拍进度页冒出「上一步」），故每个页面显示时都再调一次。
Function MeowoHideChrome
  !insertmacro MeowoHideDlgItem 1
  !insertmacro MeowoHideDlgItem 2
  !insertmacro MeowoHideDlgItem 3
  !insertmacro MeowoHideDlgItem 1028
  !insertmacro MeowoHideDlgItem 1034
  !insertmacro MeowoHideDlgItem 1035
  !insertmacro MeowoHideDlgItem 1036
  !insertmacro MeowoHideDlgItem 1037
  !insertmacro MeowoHideDlgItem 1038
  !insertmacro MeowoHideDlgItem 1045
  !insertmacro MeowoHideDlgItem 1256
FunctionEnd

; 量 client 区填 MeowoW/H，并让 1018 占位、当前自绘页、背景控件跟着铺满。
Function MeowoFitClient
  System::Call "*(i 0, i 0, i 0, i 0) p .r4"
  System::Call "user32::GetClientRect(p $HWNDPARENT, p r4)"
  System::Call "*$4(i, i, i .r5, i .r6)"
  System::Free $4
  StrCpy $MeowoW $5
  StrCpy $MeowoH $6
  GetDlgItem $0 $HWNDPARENT 1018
  System::Call "user32::MoveWindow(p r0, i 0, i 0, i r5, i r6, i 1)"
  ${If} $MeowoPage != ""
    System::Call "user32::MoveWindow(p $MeowoPage, i 0, i 0, i r5, i r6, i 1)"
  ${EndIf}
  ${If} $MeowoBgCtl != ""
    System::Call "user32::MoveWindow(p $MeowoBgCtl, i 0, i 0, i r5, i r6, i 1)"
  ${EndIf}
FunctionEnd

; 把 client 高度改成 $0（逻辑 px），左上角不动（展开时窗口向下长）。
Function MeowoSetClientHeight
  ${MeowoScale} $1 $0
  IntOp $1 $1 + $MeowoFrameH
  ${MeowoScale} $0 ${MEOWO_W}
  IntOp $0 $0 + $MeowoFrameW
  ; SWP_NOMOVE|SWP_NOZORDER|SWP_NOACTIVATE = 0x16。
  System::Call "user32::SetWindowPos(p $HWNDPARENT, p 0, i 0, i 0, i r0, i r1, i 0x16)"
  Call MeowoFitClient
FunctionEnd

; 背景位图控件补 WS_CLIPSIBLINGS（0x04000000）：它在 z 序最底，但没有这个样式时重绘（换图、
; 窗口长高）会把压在上面的文字控件整块盖掉，而文字控件不会跟着重画（实拍：展开后标语、
; 按钮字、选项文字全没了）。
Function MeowoClipBg
  System::Call "user32::GetWindowLongW(p $MeowoBgCtl, i -16) i .r0"
  IntOp $0 $0 | 0x04000000
  System::Call "user32::SetWindowLongW(p $MeowoBgCtl, i -16, i r0)"
FunctionEnd

; 自绘页补 WS_CLIPCHILDREN（0x02000000）：nsDialogs 建的页对话框没有这个样式，页面自己擦背景时会
; 把压在上面的文字控件一起涂掉，而文字控件不一定跟着重画（实拍：展开、点勾选后标语和按钮字时有时无）。
Function MeowoClipPage
  System::Call "user32::GetWindowLongW(p $MeowoPage, i -16) i .r0"
  IntOp $0 $0 | 0x02000000
  System::Call "user32::SetWindowLongW(p $MeowoPage, i -16, i r0)"
FunctionEnd

; 整窗连同所有子控件重画（RDW_INVALIDATE|RDW_ERASE|RDW_ALLCHILDREN|RDW_UPDATENOW = 0x185）。
; 只 InvalidateRect 父窗口不会刷新子控件。
Function MeowoRedrawAll
  System::Call "user32::RedrawWindow(p $HWNDPARENT, p 0, p 0, i 0x185)"
FunctionEnd

; 只释出当前 DPI 档位那一套勾选位图（File 必须在编译期写死，故每档一个分支）。
!macro MeowoToggleFiles dpi
  ${If} $MeowoUiDpi = ${dpi}
    File "/oname=$PLUGINSDIR\meowo-toggle-on.bmp" "${MEOWO_UI_DIR}\toggle-on-${dpi}.bmp"
    File "/oname=$PLUGINSDIR\meowo-toggle-off.bmp" "${MEOWO_UI_DIR}\toggle-off-${dpi}.bmp"
  ${EndIf}
!macroend

; 背景图释出到 $PLUGINSDIR（主页与 INSTFILES 都要用；passive 跳过主页，故两处都调）。
Function MeowoEnsureBg
  InitPluginsDir
  ${IfNot} ${FileExists} "$PLUGINSDIR\meowo-bg.bmp"
    File "/oname=$PLUGINSDIR\meowo-bg.bmp" "${MEOWO_BG_PATH}"
    File "/oname=$PLUGINSDIR\meowo-bg-btn.bmp" "${MEOWO_BGBTN_PATH}"
    File "/oname=$PLUGINSDIR\meowo-bg-expanded.bmp" "${MEOWO_BGEXP_PATH}"
    !insertmacro MeowoToggleFiles 96
    !insertmacro MeowoToggleFiles 120
    !insertmacro MeowoToggleFiles 144
    !insertmacro MeowoToggleFiles 168
    !insertmacro MeowoToggleFiles 192
    !insertmacro MeowoToggleFiles 240
    !insertmacro MeowoToggleFiles 288
  ${EndIf}
FunctionEnd

; 主页 / 完成页的主按钮：胶囊烙在底图里（原生按钮的白底灰边在暗色下扎眼，owner-draw 在裸
; NSIS 不可行），这里只叠一条同色可点文字带，落在胶囊直边区内（x 210..430，避开两端半圆）。
; Enter 仍走被隐藏的默认「下一步」按钮，键盘可达性不丢。
!macro MeowoPrimaryButton out text onclick
  ${MeowoText} ${out} 210 240 220 44 "${SS_CENTER}|${SS_CENTERIMAGE}|${SS_NOTIFY}" "${text}" $MeowoBtnFont ${MEOWO_COL_ONLINK} ${MEOWO_COL_LINK}
  ${NSD_OnClick} ${out} ${onclick}
!macroend

; ===== 旧版检测（上游 PageReinstall 的纯逻辑段，UI 剥离）=====
; 输出：$MeowoHasExisting(0/1)、$WixMode、$R6(WiX 卸载键)、$R0(0=同版/1=升级/-1=降级)。
; 寄存器用法与上游逐字一致；$R0 必须活到 MeowoApplyReinstall。
Function MeowoDetectExisting
  StrCpy $MeowoHasExisting 0
  ; Uninstall previous WiX installation if exists.
  ;
  ; A WiX installer stores the installation info in registry
  ; using a UUID and so we have to loop through all keys under
  ; `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall`
  ; and check if `DisplayName` and `Publisher` keys match ${PRODUCTNAME} and ${MANUFACTURER}
  StrCpy $0 0
  wix_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" wix_loop_done ; Exit loop if there is no more keys to loop on
    IntOp $0 $0 + 1
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "Publisher"
    StrCmp "$R0$R1" "${PRODUCTNAME}${MANUFACTURER}" 0 wix_loop
    ReadRegStr $R0 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    ${StrCase} $R1 $R0 "L"
    ${StrLoc} $R0 $R1 "msiexec" ">"
    StrCmp $R0 0 0 wix_loop_done
    StrCpy $WixMode 1
    StrCpy $R6 "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1"
    Goto compare_version
  wix_loop_done:

  ; Check if there is an existing installation
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" ""
  ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${IfThen} "$R0$R1" == "" ${|} Return ${|}

  compare_version:
  ${If} $WixMode = 1
    ReadRegStr $R0 HKLM "$R6" "DisplayVersion"
  ${Else}
    ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${EndIf}
  nsis_tauri_utils::SemverCompare "${VERSION}" $R0
  Pop $R0
  ${If} $R0 = 0
  ${OrIf} $R0 = 1
  ${OrIf} $R0 = -1
    StrCpy $MeowoHasExisting 1
  ${EndIf}
  ; 比较结果异常（如旧版 DisplayVersion 缺失）按无旧装处理——与上游 Abort 跳过
  ; 问询页、直接覆盖装的最终效果一致。
FunctionEnd

; ===== 旧版处置（上游 PageLeaveReinstall，问询单选换成固定策略）=====
; 同版/升级 → 直接覆盖装：与 updater 的 /UPDATE 路径同构，且 PREINSTALL hook 的
; 杀进程 + rename-aside 已为覆盖写加固，文件集固定无残留风险。
; 降级 → 先跑旧卸载器：新版本文件残留给旧版本才真正危险（上游降级路径的默认选项）。
; WiX 旧装 → 恒先卸载（历史迁移语义，上游一致）。出错 MessageBox 后 Abort 回主页面。
Function MeowoApplyReinstall
  ${If} $MeowoHasExisting = 0
    Return
  ${EndIf}
  ${If} $WixMode = 1
    Goto reinst_uninstall
  ${EndIf}
  ; In update mode, always proceeds without uninstalling
  ${If} $UpdateMode = 1
    Goto reinst_done
  ${EndIf}
  ${If} $R0 = 0
  ${OrIf} $R0 = 1
    Goto reinst_done
  ${EndIf}

  reinst_uninstall:
    HideWindow
    ClearErrors

    ${If} $WixMode = 1
      ReadRegStr $R1 HKLM "$R6" "UninstallString"
      ExecWait '$R1' $0
    ${Else}
      ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
      ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
      ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|} ; append /UPDATE
      ${IfThen} $PassiveMode = 1 ${|} StrCpy $R1 "$R1 /P" ${|} ; append /P
      StrCpy $R1 "$R1 _?=$4" ; append uninstall directory
      ExecWait '$R1' $0
    ${EndIf}

    BringToFront

    ${IfThen} ${Errors} ${|} StrCpy $0 2 ${|} ; ExecWait failed, set fake exit code

    ${If} $0 <> 0
    ${OrIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ; User cancelled wix uninstaller? return to the main page
      ${If} $WixMode = 1
      ${AndIf} $0 = 1602
        Abort
      ${EndIf}

      ; User cancelled NSIS uninstaller? return to the main page
      ${If} $0 = 1
        Abort
      ${EndIf}

      ; Other errors? show generic error message and return to the main page
      MessageBox MB_ICONEXCLAMATION "$(unableToUninstall)"
      Abort
    ${EndIf}
  reinst_done:
FunctionEnd

; ===== 自绘主页：品牌区 → 标语 → 主按钮 → 紧贴其下的「自定义安装选项」，展开时窗口向下长出选项卡片 =====
Page custom MeowoMainPage MeowoMainLeave

Function MeowoMainPage
  Call MeowoDetectExisting
  ; passive（updater 静默升级）：复刻上游行为——不建页面，直接跑重装决策后放行；
  ; 不创建对话框即自动跳过本页（上游 PageReinstall 的同款路径）。
  ${If} $PassiveMode = 1
    Call MeowoApplyReinstall
    Return
  ${EndIf}

  Call MeowoEnsureBg
  Call MeowoHideChrome
  nsDialogs::Create 1018
  Pop $MeowoPage
  ${If} $MeowoPage == error
    Abort
  ${EndIf}
  SetCtlColors $MeowoPage "${MEOWO_COL_TEXT}" "${MEOWO_COL_BASE}"
  Call MeowoClipPage

  ; 注意：背景位图在本函数**最末**创建（nsDialogs::Show 之前）。nsDialogs 的 z 序
  ; 即创建序——先建在顶、后建垫底（Z 走查实锤；SetWindowPos(HWND_BOTTOM) 在
  ; nsDialogs 页上会被吞）。位图先建就会盖住并吞掉所有控件的点击（实拍翻车过）。
  ; 图标烙在底图里（y 56..128），字标用字体渲染（理由见 generate-nsis-bitmaps.py 的 _brand）。
  ${MeowoText} $0 0 138 ${MEOWO_W} 40 "${SS_CENTER}" "Meowo" $MeowoWordFont ${MEOWO_COL_TEXT} ${MEOWO_COL_BASE}

  ${MeowoText} $0 0 182 ${MEOWO_W} 22 "${SS_CENTER}" "$(meowoTagline)" $MeowoBodyFont ${MEOWO_COL_SUB} ${MEOWO_COL_BASE}

  ; 旧版提示（透明度换信任——一键化省掉了问询页，但要说清会发生什么）。
  ${MeowoText} $MeowoNoteLabel 0 206 ${MEOWO_W} 18 "${SS_CENTER}" "" $MeowoSmallFont ${MEOWO_COL_FAINT} ${MEOWO_COL_BASE}
  ${If} $MeowoHasExisting = 1
    ${If} $R0 = 1
      ${NSD_SetText} $MeowoNoteLabel "$(meowoNoteUpgrade)"
    ${ElseIf} $R0 = -1
      ${NSD_SetText} $MeowoNoteLabel "$(meowoNoteDowngrade)"
    ${Else}
      ${NSD_SetText} $MeowoNoteLabel "$(meowoNoteSame)"
    ${EndIf}
  ${EndIf}

  !insertmacro MeowoPrimaryButton $0 "$(meowoOneClick)" MeowoOnInstallClick

  ; 次级入口紧贴主按钮下方居中（原先缩在右下角，与主按钮没有关系）。
  ${MeowoText} $MeowoCustomLink 220 298 200 20 "${SS_CENTER}|${SS_NOTIFY}" "$(meowoCustomOpen)" $MeowoSmallFont ${MEOWO_COL_SUB} ${MEOWO_COL_BASE}
  ${NSD_OnClick} $MeowoCustomLink MeowoOnToggleCustom

  ; 左下角版本号（展开时被选项卡片占位，随之隐藏）。
  ${MeowoText} $MeowoVersionLabel 20 372 240 16 "0" "$(meowoVersion)" $MeowoSmallFont ${MEOWO_COL_FAINT} ${MEOWO_COL_BASE}

  ; ---- 选项卡片（默认隐藏；卡片 32,340 576x128，输入框 52,378 536x34，勾选框 52,432 18x18）----
  ; 2026-11-20 起不再提供「对话窗口功能」开关：一律按完整模式安装（leader），轻量模式只在设置里切。
  ${MeowoText} $MeowoDirLabel 52 354 300 18 "0" "$(meowoInstallPath)" $MeowoSmallFont ${MEOWO_COL_SUB} ${MEOWO_COL_CARD}

  ; 路径输入：无边框 EDIT 嵌在烙好的圆角输入框里（原生带边框的白底输入框在暗色下像另一个程序）。
  ${MeowoScale} $2 64
  ${MeowoScale} $3 386
  ${MeowoScale} $4 476
  ${MeowoScale} $5 18
  nsDialogs::CreateControl EDIT "${DEFAULT_STYLES}|${WS_TABSTOP}|${ES_AUTOHSCROLL}" 0 $2 $3 $4 $5 "$INSTDIR"
  Pop $MeowoDirText
  SetCtlColors $MeowoDirText "${MEOWO_COL_TEXT}" "${MEOWO_COL_FIELD}"
  SendMessage $MeowoDirText ${WM_SETFONT} $MeowoSmallFont 1

  ; 选文件夹：输入框右端一个文件夹图标（Segoe MDL2 Assets 的 OpenFolderHorizontal 字形 U+ED25（U+E8B7 在这款字体里是一张纸，不像文件夹）），替代「更改」文字。
  ${MeowoText} $MeowoChangeLink 550 384 28 22 "${SS_CENTER}|${SS_CENTERIMAGE}|${SS_NOTIFY}" "${U+ED25}" $MeowoIconFont ${MEOWO_COL_SUB} ${MEOWO_COL_FIELD}
  ${NSD_OnClick} $MeowoChangeLink MeowoOnBrowse

  ; 「创建桌面快捷方式」（语义 = 上游 FINISH 页那颗，建档时机挪进 Section）。勾选框是位图控件，
  ; 两态位图按 DPI 档位加载一次（LR_LOADFROMFILE = 0x10，原尺寸），切换时只换图。
  System::Call "user32::LoadImage(p 0, t '$PLUGINSDIR\meowo-toggle-on.bmp', i 0, i 0, i 0, i 0x10) p .s"
  Pop $MeowoToggleOn
  System::Call "user32::LoadImage(p 0, t '$PLUGINSDIR\meowo-toggle-off.bmp', i 0, i 0, i 0, i 0x10) p .s"
  Pop $MeowoToggleOff
  ${MeowoScale} $2 52
  ${MeowoScale} $3 432
  ${MeowoScale} $4 18
  nsDialogs::CreateControl STATIC "${DEFAULT_STYLES}|${SS_BITMAP}|${SS_NOTIFY}" 0 $2 $3 $4 $4 ""
  Pop $MeowoDesktopBox
  ${NSD_OnClick} $MeowoDesktopBox MeowoOnDesktopToggle
  ${MeowoText} $MeowoDesktopLabel 80 431 480 20 "${SS_NOTIFY}" "$(meowoDesktopLnk)" $MeowoSmallFont ${MEOWO_COL_TEXT} ${MEOWO_COL_CARD}
  ${NSD_OnClick} $MeowoDesktopLabel MeowoOnDesktopToggle

  ; 勾选状态来自 .onInit 写好的产品默认（oneclick-defaults）；从别页返回时保留用户的选择。
  ${MeowoPaintToggle} $MeowoDesktopBox $MeowoDesktopLnk

  StrCpy $MeowoExpanded 0
  StrCpy $1 ${SW_HIDE}
  Call MeowoShowOptions

  ; 背景位图最后创建 = z 序垫底（理由见函数开头注释）。单张 2x 图缩小采样。
  ${NSD_CreateBitmap} 0 0 100% 100% ""
  Pop $MeowoBgCtl
  Call MeowoClipBg
  ${NSD_SetStretchedImage} $MeowoBgCtl "$PLUGINSDIR\meowo-bg-btn.bmp" $MeowoBgHandle

  nsDialogs::Show
  ; 页面销毁后句柄失效，别让后面的 MeowoFitClient 去挪已不存在的窗口。
  StrCpy $MeowoPage ""
  StrCpy $MeowoBgCtl ""
FunctionEnd

; 选项卡片里的控件一起显隐（$1 = SW_SHOW / SW_HIDE）。
Function MeowoShowOptions
  ShowWindow $MeowoDirLabel $1
  ShowWindow $MeowoDirText $1
  ShowWindow $MeowoChangeLink $1
  ShowWindow $MeowoDesktopBox $1
  ShowWindow $MeowoDesktopLabel $1
FunctionEnd

Function MeowoOnInstallClick
  Pop $0
  ; 驱动被隐藏的「下一步」(ID 1)：走标准页流，MeowoMainLeave 校验照常触发。
  SendMessage $HWNDPARENT ${WM_COMMAND} 1 0
FunctionEnd

; 展开 / 收起选项卡片：窗口向下长出（或缩回）MEOWO_EXPAND，换对应底图，卡片控件显隐。
Function MeowoOnToggleCustom
  Pop $0
  ${If} $MeowoExpanded = 0
    StrCpy $MeowoExpanded 1
    IntOp $0 ${MEOWO_H} + ${MEOWO_EXPAND}
    Call MeowoSetClientHeight
    ${NSD_FreeImage} $MeowoBgHandle
    ${NSD_SetStretchedImage} $MeowoBgCtl "$PLUGINSDIR\meowo-bg-expanded.bmp" $MeowoBgHandle
    ${NSD_SetText} $MeowoCustomLink "$(meowoCustomClose)"
    ShowWindow $MeowoVersionLabel ${SW_HIDE}
    StrCpy $1 ${SW_SHOW}
  ${Else}
    StrCpy $MeowoExpanded 0
    StrCpy $1 ${SW_HIDE}
    Call MeowoShowOptions
    StrCpy $0 ${MEOWO_H}
    Call MeowoSetClientHeight
    ${NSD_FreeImage} $MeowoBgHandle
    ${NSD_SetStretchedImage} $MeowoBgCtl "$PLUGINSDIR\meowo-bg-btn.bmp" $MeowoBgHandle
    ${NSD_SetText} $MeowoCustomLink "$(meowoCustomOpen)"
    ShowWindow $MeowoVersionLabel ${SW_SHOW}
    StrCpy $1 ${SW_HIDE}
  ${EndIf}
  Call MeowoShowOptions
  Call MeowoRedrawAll
FunctionEnd

Function MeowoOnDesktopToggle
  Pop $0
  IntOp $MeowoDesktopLnk 1 - $MeowoDesktopLnk
  ${MeowoPaintToggle} $MeowoDesktopBox $MeowoDesktopLnk
FunctionEnd

Function MeowoOnBrowse
  Pop $0
  ${NSD_GetText} $MeowoDirText $1
  nsDialogs::SelectFolderDialog "$(meowoInstallPath)" $1
  Pop $1
  ${If} $1 != error
    ; 末段不是产品名则追加（对齐上游 DIRECTORY 页的目录语义）。
    StrLen $2 "${PRODUCTNAME}"
    StrCpy $3 $1 "" -$2
    ${If} $3 != "${PRODUCTNAME}"
      StrCpy $1 "$1\${PRODUCTNAME}"
    ${EndIf}
    ${NSD_SetText} $MeowoDirText $1
  ${EndIf}
FunctionEnd

Function MeowoMainLeave
  ; 勾选值已由 MeowoOnDesktopToggle 实时写进 $MeowoDesktopLnk。
  ${NSD_GetText} $MeowoDirText $0
  ${If} $0 == ""
    MessageBox MB_ICONEXCLAMATION "$(meowoBadPath)"
    Abort
  ${EndIf}
  StrCpy $INSTDIR $0
  Call MeowoApplyReinstall
  ; 后面的进度页、完成页按收起态尺寸排版：展开着就先缩回去（放在可能 Abort 的校验之后，
  ; 回到本页时卡片仍是展开的样子）。
  ${If} $MeowoExpanded = 1
    StrCpy $MeowoExpanded 0
    StrCpy $1 ${SW_HIDE}
    Call MeowoShowOptions
    StrCpy $0 ${MEOWO_H}
    Call MeowoSetClientHeight
  ${EndIf}
FunctionEnd
; MEOWO-END oneclick-ui

; 2. License Page (if defined)
!if "${LICENSE}" != ""
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MUI_PAGE_LICENSE "${LICENSE}"
!endif

; 3. Install mode (if it is set to `both`)
!if "${INSTALLMODE}" == "both"
  !define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
  !insertmacro MULTIUSER_PAGE_INSTALLMODE
!endif

; MEOWO-BEGIN reinstall-removed
; 4. 上游的重装问询页（PageReinstall / PageLeaveReinstall）已迁移为纯逻辑函数
;    MeowoDetectExisting / MeowoApplyReinstall（见上方 oneclick-ui 块），
;    问询单选换成固定策略，理由见彼处注释。
; MEOWO-END reinstall-removed

; MEOWO-BEGIN pages-directory-startmenu
; 5. DIRECTORY 页并入自绘主页的「自定义安装」展开区（路径输入 + 浏览）。
; 6. STARTMENU 页移除：本项目 STARTMENUFOLDER 恒空，该页在上游也恒被 Skip；
;    $AppStartMenuFolder 在 .onInit 里按配置回填，Section 直接建快捷方式。
Var AppStartMenuFolder
; MEOWO-END pages-directory-startmenu

; 7. Installation page
; MEOWO-BEGIN instfiles-restyle
!define MUI_PAGE_CUSTOMFUNCTION_SHOW MeowoInstFilesShow
!insertmacro MUI_PAGE_INSTFILES

; INSTFILES 只能由 Section 驱动（NSIS 没有「custom 页后台跑 Section」的机制），
; 故不另起炉灶，就地改造：隐藏细节列表与原状态行，自绘字标 + 状态文案 + 细进度条，背景图压 z 序最底。
Function MeowoInstFilesShow
  Call MeowoEnsureBg
  Call MeowoHideChrome
  FindWindow $1 "#32770" "" $HWNDPARENT
  ; 页面对话框本身铺满 client 区：MUI 按原始尺寸建内页，不铺满的话按整窗坐标摆的
  ; 进度条/状态行落在小对话框外被裁掉，背景只画出左上一块、其余露系统灰底（实拍）。
  System::Call "user32::MoveWindow(p r1, i 0, i 0, i $MeowoW, i $MeowoH, i 1)"
  GetDlgItem $0 $1 1016    ; 细节列表
  ShowWindow $0 ${SW_HIDE}
  GetDlgItem $0 $1 1027    ; 「显示细节」按钮
  ShowWindow $0 ${SW_HIDE}

  ; 状态行：MUI 的 1006 不吃 SetCtlColors（INSTFILES 页的对话框过程不按我们的配色画它，
  ; 实拍是一块系统浅灰底），藏掉它，换成自己的一行固定文案。代价是看不到逐文件的
  ; DetailPrint（含 WebView2 下载提示）；进度条照常走。
  GetDlgItem $0 $1 1006
  ShowWindow $0 ${SW_HIDE}
  ${MeowoScale} $3 208
  ${MeowoScale} $5 20
  ; WS_CHILD|WS_VISIBLE|SS_CENTER = 0x50000001
  System::Call "user32::CreateWindowEx(i 0, t 'STATIC', t '$(meowoInstalling)', i 0x50000001, i 0, i r3, i $MeowoW, i r5, p $1, p 0, p 0, p 0) p .r0"
  SetCtlColors $0 "${MEOWO_COL_SUB}" "${MEOWO_COL_BASE}"
  SendMessage $0 ${WM_SETFONT} $MeowoSmallFont 1

  ; 进度条：y=244，320x6 居中的细条。去掉视觉样式与边框后才能自定颜色——
  ; themed 进度条恒为系统绿配浅灰轨道（实拍），PBM_SETBARCOLOR 只对无主题控件生效。
  GetDlgItem $0 $1 1004
  System::Call "uxtheme::SetWindowTheme(p r0, w '', w '')"
  System::Call "user32::GetWindowLongW(p r0, i -16) i .r2"
  IntOp $2 $2 & 0xFF7FFFFF                  ; 去 WS_BORDER
  System::Call "user32::SetWindowLongW(p r0, i -16, i r2)"
  System::Call "user32::GetWindowLongW(p r0, i -20) i .r2"
  IntOp $2 $2 & 0xFFFDFDFF                  ; 去 WS_EX_STATICEDGE(0x20000) / WS_EX_CLIENTEDGE(0x200)
  System::Call "user32::SetWindowLongW(p r0, i -20, i r2)"
  ${MeowoScale} $2 160
  ${MeowoScale} $3 244
  ${MeowoScale} $4 320
  ${MeowoScale} $5 6
  ; SWP_FRAMECHANGED|SWP_NOZORDER|SWP_NOACTIVATE = 0x34：边框样式改动要靠 FRAMECHANGED 生效。
  System::Call "user32::SetWindowPos(p r0, p 0, i r2, i r3, i r4, i r5, i 0x34)"
  SendMessage $0 0x0409 0 0x004CF6D4         ; PBM_SETBARCOLOR：青柠 d4f64c（COLORREF = 0x00BBGGRR）
  SendMessage $0 0x2001 0 0x002F2A2A         ; PBM_SETBKCOLOR：轨道 2a2a2f

  ; 字标：同主页几何，SS_CENTER 的 STATIC（WS_CHILD|WS_VISIBLE|SS_CENTER = 0x50000001）。
  ${MeowoScale} $3 138
  ${MeowoScale} $5 40
  System::Call "user32::CreateWindowEx(i 0, t 'STATIC', t 'Meowo', i 0x50000001, i 0, i r3, i $MeowoW, i r5, p $1, p 0, p 0, p 0) p .r0"
  SetCtlColors $0 "${MEOWO_COL_TEXT}" "${MEOWO_COL_BASE}"
  SendMessage $0 ${WM_SETFONT} $MeowoWordFont 1

  ; 背景图：SS_BITMAP static（WS_CHILD|WS_VISIBLE|WS_CLIPSIBLINGS|SS_BITMAP = 0x5400000E），
  ; LR_LOADFROMFILE=0x10 按 client 尺寸缩放加载，SetWindowPos 压到 HWND_BOTTOM。
  ; WS_CLIPSIBLINGS 不能省：没有它，背景重绘时会把压在上面的状态行整块盖掉（状态行只在文字
  ; 变化时重绘，实拍进度页一直看不到状态行）。
  System::Call "user32::LoadImage(p 0, t '$PLUGINSDIR\meowo-bg.bmp', i 0, i $MeowoW, i $MeowoH, i 0x10) p .r6"
  System::Call "user32::CreateWindowEx(i 0, t 'STATIC', t '', i 0x5400000E, i 0, i 0, i $MeowoW, i $MeowoH, p $1, p 0, p 0, p 0) p .r7"
  SendMessage $7 0x172 0 $6 ; STM_SETIMAGE(IMAGE_BITMAP)
  System::Call "user32::SetWindowPos(p r7, p 1, i 0, i 0, i 0, i 0, i 0x13)"
FunctionEnd
; MEOWO-END instfiles-restyle

; 8. Finish page
; MEOWO-BEGIN done-page
; 上游 FINISH 页换成同视觉体系的自绘完成页：桌面快捷方式勾选已挪进主页
; （建档时机随之挪进 Section），这里零决策——一颗「立即体验」+ 其下一个「关闭」链接。
Page custom MeowoDonePage

Function RunMainBinary
  nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
FunctionEnd

Function MeowoDonePage
  ; passive 跳过（等价上游 FINISH 的 SkipIfPassive；自动关窗由 Section 的 SetAutoClose 承担）。
  ${If} $PassiveMode = 1
    Return
  ${EndIf}
  Call MeowoEnsureBg
  Call MeowoHideChrome
  nsDialogs::Create 1018
  Pop $MeowoPage
  ${If} $MeowoPage == error
    Abort
  ${EndIf}
  SetCtlColors $MeowoPage "${MEOWO_COL_TEXT}" "${MEOWO_COL_BASE}"
  Call MeowoClipPage

  ; 同主页：背景位图在本函数最末创建（z 序 = 创建序，后建垫底）。
  ; 字标同主页；「安装完成」占主页标语 + 提示两行的位置，主按钮与次级链接与主页同几何。
  ${MeowoText} $0 0 138 ${MEOWO_W} 40 "${SS_CENTER}" "Meowo" $MeowoWordFont ${MEOWO_COL_TEXT} ${MEOWO_COL_BASE}
  ${MeowoText} $0 0 184 ${MEOWO_W} 32 "${SS_CENTER}" "$(meowoDoneTitle)" $MeowoTitleFont ${MEOWO_COL_TEXT} ${MEOWO_COL_BASE}
  !insertmacro MeowoPrimaryButton $0 "$(meowoLaunch)" MeowoOnLaunchClick
  ; 同主页链接：不能用 NSD_CreateLink（首次获焦误触发点击——在这页等于装完瞬间关窗）。
  ${MeowoText} $0 220 298 200 20 "${SS_CENTER}|${SS_NOTIFY}" "$(meowoFinish)" $MeowoSmallFont ${MEOWO_COL_SUB} ${MEOWO_COL_BASE}
  ${NSD_OnClick} $0 MeowoOnFinishClick

  ${NSD_CreateBitmap} 0 0 100% 100% ""
  Pop $MeowoBgCtl
  Call MeowoClipBg
  ${NSD_SetStretchedImage} $MeowoBgCtl "$PLUGINSDIR\meowo-bg-btn.bmp" $MeowoBgHandle

  nsDialogs::Show
  StrCpy $MeowoPage ""
  StrCpy $MeowoBgCtl ""
FunctionEnd

Function MeowoOnLaunchClick
  Pop $0
  Call RunMainBinary
  SendMessage $HWNDPARENT ${WM_COMMAND} 1 0 ; 最后一页的「下一步」= 关闭安装器
FunctionEnd

Function MeowoOnFinishClick
  Pop $0
  SendMessage $HWNDPARENT ${WM_COMMAND} 1 0
FunctionEnd
; MEOWO-END done-page

; Uninstaller Pages
; 1. Confirm uninstall page
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState
!define /ifndef WS_EX_LAYOUTRTL         0x00400000
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
Function un.ConfirmShow ; Add add a `Delete app data` check box
  ; $1 inner dialog HWND
  ; $2 window DPI
  ; $3 style
  ; $4 x
  ; $5 y
  ; $6 width
  ; $7 height
  FindWindow $1 "#32770" "" $HWNDPARENT ; Find inner dialog
  System::Call "user32::GetDpiForWindow(p r1) i .r2"
  ${If} $(^RTL) = 1
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE} | ${WS_EX_LAYOUTRTL}"
    IntOp $4 50 * $2
  ${Else}
    StrCpy $3 "${__NSD_CheckBox_EXSTYLE}"
    IntOp $4 0 * $2
  ${EndIf}
  IntOp $5 100 * $2
  IntOp $6 400 * $2
  IntOp $7 25 * $2
  IntOp $4 $4 / 96
  IntOp $5 $5 / 96
  IntOp $6 $6 / 96
  IntOp $7 $7 / 96
  System::Call 'user32::CreateWindowEx(i r3, w "${__NSD_CheckBox_CLASS}", w "$(deleteAppData)", i ${__NSD_CheckBox_STYLE}, i r4, i r5, i r6, i r7, p r1, i0, i0, i0) i .s'
  Pop $DeleteAppDataCheckbox
  SendMessage $HWNDPARENT ${WM_GETFONT} 0 0 $1
  SendMessage $DeleteAppDataCheckbox ${WM_SETFONT} $1 1
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
Function un.ConfirmLeave
  SendMessage $DeleteAppDataCheckbox ${BM_GETCHECK} 0 0 $DeleteAppDataCheckboxState
FunctionEnd
!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM

; 2. Uninstalling Page
!insertmacro MUI_UNPAGE_INSTFILES

;Languages
{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
!insertmacro MUI_RESERVEFILE_LANGDLL
{{#each language_files}}
  !include "{{this}}"
{{/each}}

; MEOWO-BEGIN oneclick-langstrings
; 自绘页文案。项目语言固定为 SimpChinese + English（tauri.conf.json 的
; bundle.windows.nsis.languages）；增删语言时必须同步这里，否则编译期报未定义 LANG_*。
LangString meowoTagline ${LANG_SIMPCHINESE} "在一个地方运行你所有的 AI 编程会话"
LangString meowoTagline ${LANG_ENGLISH} "Run all your AI coding sessions in one place"
LangString meowoOneClick ${LANG_SIMPCHINESE} "立即安装"
LangString meowoOneClick ${LANG_ENGLISH} "Install Now"
LangString meowoCustomOpen ${LANG_SIMPCHINESE} "自定义安装选项"
LangString meowoCustomOpen ${LANG_ENGLISH} "Install options"
LangString meowoCustomClose ${LANG_SIMPCHINESE} "收起选项"
LangString meowoCustomClose ${LANG_ENGLISH} "Hide options"
LangString meowoInstallPath ${LANG_SIMPCHINESE} "安装位置"
LangString meowoInstallPath ${LANG_ENGLISH} "Install to"
LangString meowoDesktopLnk ${LANG_SIMPCHINESE} "创建桌面快捷方式"
LangString meowoDesktopLnk ${LANG_ENGLISH} "Create desktop shortcut"
LangString meowoNoteSame ${LANG_SIMPCHINESE} "检测到相同版本已安装，将原地重新安装"
LangString meowoNoteSame ${LANG_ENGLISH} "This version is already installed — it will be reinstalled in place"
LangString meowoNoteUpgrade ${LANG_SIMPCHINESE} "检测到已安装的旧版本，将覆盖升级"
LangString meowoNoteUpgrade ${LANG_ENGLISH} "An older version is installed — it will be upgraded in place"
LangString meowoNoteDowngrade ${LANG_SIMPCHINESE} "检测到更新的版本，安装前会先将其卸载"
LangString meowoNoteDowngrade ${LANG_ENGLISH} "A newer version is installed — it will be uninstalled first"
LangString meowoBadPath ${LANG_SIMPCHINESE} "请填写有效的安装位置"
LangString meowoBadPath ${LANG_ENGLISH} "Please enter a valid install location"
LangString meowoDoneTitle ${LANG_SIMPCHINESE} "安装完成"
LangString meowoDoneTitle ${LANG_ENGLISH} "All set"
LangString meowoLaunch ${LANG_SIMPCHINESE} "立即体验"
LangString meowoLaunch ${LANG_ENGLISH} "Launch Meowo"
LangString meowoFinish ${LANG_SIMPCHINESE} "关闭"
LangString meowoFinish ${LANG_ENGLISH} "Close"
LangString meowoInstalling ${LANG_SIMPCHINESE} "正在安装 Meowo…"
LangString meowoInstalling ${LANG_ENGLISH} "Installing Meowo…"
LangString meowoVersion ${LANG_SIMPCHINESE} "版本 ${VERSION}"
LangString meowoVersion ${LANG_ENGLISH} "Version ${VERSION}"
; MEOWO-END oneclick-langstrings

Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/NS" $NoShortcutMode
  ${IfNot} ${Errors}
    StrCpy $NoShortcutMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}

  ; MEOWO-BEGIN oneclick-defaults
  ; 勾选默认值：主页 leave 不执行（passive/silent）时 Section 仍能读到产品默认。
  StrCpy $MeowoDesktopLnk 1
  ; STARTMENU 页已移除，按配置回填其变量（本项目恒空 → 快捷方式建在 $SMPROGRAMS 根）。
  !if "${STARTMENUFOLDER}" != ""
    StrCpy $AppStartMenuFolder "${STARTMENUFOLDER}"
  !endif
  ; MEOWO-END oneclick-defaults

  !if "${DISPLAYLANGUAGESELECTOR}" == "true"
    !insertmacro MUI_LANGDLL_DISPLAY
  !endif

  !insertmacro SetContext

  ${If} $INSTDIR == "${PLACEHOLDER_INSTALL_DIR}"
    ; Set default install location
    !if "${INSTALLMODE}" == "perMachine"
      ${If} ${RunningX64}
        !if "${ARCH}" == "x64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else if "${ARCH}" == "arm64"
          StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
        !else
          StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
        !endif
      ${Else}
        StrCpy $INSTDIR "$PROGRAMFILES\${PRODUCTNAME}"
      ${EndIf}
    !else if "${INSTALLMODE}" == "currentUser"
      StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
    !endif

    Call RestorePreviousInstallLocation
  ${EndIf}


  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_INIT
  !endif
FunctionEnd


Section EarlyChecks
  ; Abort silent installer if downgrades is disabled
  !if "${ALLOWDOWNGRADES}" == "false"
  ${If} ${Silent}
    ; If downgrading
    ${If} $R0 = -1
      System::Call 'kernel32::AttachConsole(i -1)i.r0'
      ${If} $0 <> 0
        System::Call 'kernel32::GetStdHandle(i -11)i.r0'
        System::call 'kernel32::SetConsoleTextAttribute(i r0, i 0x0004)' ; set red color
        FileWrite $0 "$(silentDowngrades)"
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  !endif

SectionEnd

Section WebView2
  ; Check if Webview2 is already installed and skip this section
  ${If} ${RunningX64}
    ReadRegStr $4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $4 == ""
    ReadRegStr $4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}

  ${If} $4 == ""
    ; Webview2 installation
    ;
    ; Skip if updating
    ${If} $UpdateMode <> 1
      !if "${INSTALLWEBVIEW2MODE}" == "downloadBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        DetailPrint "$(webview2Downloading)"
        NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Pop $0
        ${If} $0 == "success"
          DetailPrint "$(webview2DownloadSuccess)"
        ${Else}
          DetailPrint "$(webview2DownloadError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "embedBootstrapper"
        Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2BOOTSTRAPPERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebview2Setup.exe"
        Goto install_webview2
      !endif

      !if "${INSTALLWEBVIEW2MODE}" == "offlineInstaller"
        Delete "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"
        DetailPrint "$(installingWebview2)"
        StrCpy $6 "$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe"
        Goto install_webview2
      !endif

      Goto webview2_done

      install_webview2:
        DetailPrint "$(installingWebview2)"
        ; $6 holds the path to the webview2 installer
        ExecWait "$6 ${WEBVIEW2INSTALLERARGS} /install" $1
        ${If} $1 = 0
          DetailPrint "$(webview2InstallSuccess)"
        ${Else}
          DetailPrint "$(webview2InstallError)"
          Abort "$(webview2AbortError)"
        ${EndIf}
      webview2_done:
    ${EndIf}
  ${Else}
    !if "${MINIMUMWEBVIEW2VERSION}" != ""
      ${VersionCompare} "${MINIMUMWEBVIEW2VERSION}" "$4" $R0
      ${If} $R0 = 1
        update_webview:
          DetailPrint "$(installingWebview2)"
          ${If} ${RunningX64}
            ReadRegStr $R1 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate" "path"
          ${Else}
            ReadRegStr $R1 HKLM "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 == ""
            ReadRegStr $R1 HKCU "SOFTWARE\Microsoft\EdgeUpdate" "path"
          ${EndIf}
          ${If} $R1 != ""
            ; Chromium updater docs: https://source.chromium.org/chromium/chromium/src/+/main:docs/updater/user_manual.md
            ; Modified from "HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Microsoft EdgeWebView\ModifyPath"
            ExecWait `"$R1" /install appguid=${WEBVIEW2APPGUID}&needsadmin=true` $1
            ${If} $1 = 0
              DetailPrint "$(webview2InstallSuccess)"
            ${Else}
              MessageBox MB_ICONEXCLAMATION|MB_ABORTRETRYIGNORE "$(webview2InstallError)" IDIGNORE ignore IDRETRY update_webview
              Quit
              ignore:
            ${EndIf}
          ${EndIf}
      ${EndIf}
    !endif
  ${EndIf}
SectionEnd

Section Install
  SetOutPath $INSTDIR

  !ifmacrodef NSIS_HOOK_PREINSTALL
    !insertmacro NSIS_HOOK_PREINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Copy main executable
  File "${MAINBINARYSRCPATH}"

  ; Copy resources
  {{#each resources_dirs}}
    CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
    File /a "/oname={{this.[1]}}" "{{no-escape @key}}"
  {{/each}}

  ; Copy external binaries
  {{#each binaries}}
    File /a "/oname={{this}}" "{{no-escape @key}}"
  {{/each}}

  ; Create file associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
       !insertmacro APP_ASSOCIATE "{{ext}}" "{{or association.name ext}}" "{{association-description association.description ext}}" "$INSTDIR\${MAINBINARYNAME}.exe,0" "Open with ${PRODUCTNAME}" "$INSTDIR\${MAINBINARYNAME}.exe $\"%1$\""
    {{/each}}
  {{/each}}

  ; Register deep links
  {{#each deep_link_protocols as |protocol| ~}}
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "URL Protocol" ""
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}" "" "URL:${BUNDLEID} protocol"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
    WriteRegStr SHCTX "Software\Classes\\{{protocol}}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  {{/each}}

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Save $INSTDIR in registry for future installations
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "" $INSTDIR

  !if "${INSTALLMODE}" == "both"
    ; Save install mode to be selected by default for the next installation such as updating
    ; or when uninstalling
    WriteRegStr SHCTX "${UNINSTKEY}" $MultiUser.InstallMode 1
  !endif

  ; Remove old main binary if it doesn't match new main binary name
  ReadRegStr $OldMainBinaryName SHCTX "${UNINSTKEY}" "MainBinaryName"
  ${If} $OldMainBinaryName != ""
  ${AndIf} $OldMainBinaryName != "${MAINBINARYNAME}.exe"
    Delete "$INSTDIR\$OldMainBinaryName"
  ${EndIf}

  ; Save current MAINBINARYNAME for future updates
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "${MAINBINARYNAME}.exe"

  ; Registry information for add/remove programs
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" "1"

  ${GetSize} "$INSTDIR" "/M=uninstall.exe /S=0K /G=0" $0 $1 $2
  IntOp $0 $0 + ${ESTIMATEDSIZE}
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" "$0"

  !if "${HOMEPAGE}" != ""
    WriteRegStr SHCTX "${UNINSTKEY}" "URLInfoAbout" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "URLUpdateInfo" "${HOMEPAGE}"
    WriteRegStr SHCTX "${UNINSTKEY}" "HelpLink" "${HOMEPAGE}"
  !endif

  ; MEOWO-BEGIN chat-enabled-seed
  ; 安装器不再提供「对话窗口功能」开关（2026-11-20 起一律完整模式），也就不再写 ChatEnabled 种子：
  ; 新装按应用默认（chat_enabled = true），老用户在设置里的选择原样保留。应用侧 seed.rs 照旧
  ; 消化旧版安装器留下的种子，卸载器照旧兜底清理。
  ; MEOWO-END chat-enabled-seed

  ; Create start menu shortcut
  ; MEOWO-BEGIN startmenu-direct
  ; STARTMENU 页已移除（MUI_STARTMENU_WRITE_* 依赖页面声明）；直接建快捷方式，
  ; 与上游「页面恒 Skip + 宏退化为直调」的实效一致。
  Call CreateOrUpdateStartMenuShortcut
  ; MEOWO-END startmenu-direct

  ; Create desktop shortcut for silent and passive installers
  ; because finish page will be skipped
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    Call CreateOrUpdateDesktopShortcut
  ; MEOWO-BEGIN desktop-shortcut-choice
  ; GUI：按主页「创建桌面快捷方式」勾选（上游放在 FINISH 页，勾选挪到主页后
  ; 建档时机随之挪进 Section）。/NS 与 /UPDATE 的拦截仍在函数内部，覆盖所有调用点。
  ${ElseIf} $MeowoDesktopLnk = 1
    Call CreateOrUpdateDesktopShortcut
  ; MEOWO-END desktop-shortcut-choice
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTINSTALL
    !insertmacro NSIS_HOOK_POSTINSTALL
  !endif

  ; Auto close this page for passive mode
  ; MEOWO: 改为无条件——GUI 下自动前进到自绘完成页（细节列表已隐藏，停留无意义）；
  ; passive 下完成页自跳，行为与上游等价。
  SetAutoClose true
SectionEnd

Function .onInstSuccess
  ; Check for `/R` flag only in silent and passive installers because
  ; GUI installer has a toggle for the user to (re)start the app
  ${If} $PassiveMode = 1
  ${OrIf} ${Silent}
    ${GetOptions} $CMDLINE "/R" $R0
    ${IfNot} ${Errors}
      ${GetOptions} $CMDLINE "/ARGS" $R0
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "$R0"
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro SetContext

  !if "${INSTALLMODE}" == "both"
    !insertmacro MULTIUSER_UNINIT
  !endif

  !insertmacro MUI_UNGETLANGUAGE

  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}

  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section Uninstall

  !ifmacrodef NSIS_HOOK_PREUNINSTALL
    !insertmacro NSIS_HOOK_PREUNINSTALL
  !endif

  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Delete the app directory and its content from disk
  ; Copy main executable
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"

  ; Delete resources
  {{#each resources}}
    Delete "$INSTDIR\\{{this.[1]}}"
  {{/each}}

  ; Delete external binaries
  {{#each binaries}}
    Delete "$INSTDIR\\{{this}}"
  {{/each}}

  ; Delete app associations
  {{#each file_associations as |association| ~}}
    {{#each association.ext as |ext| ~}}
      !insertmacro APP_UNASSOCIATE "{{ext}}" "{{or association.name ext}}"
    {{/each}}
  {{/each}}

  ; Delete deep links
  {{#each deep_link_protocols as |protocol| ~}}
    ReadRegStr $R7 SHCTX "Software\Classes\\{{protocol}}\shell\open\command" ""
    ${If} $R7 == "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
      DeleteRegKey SHCTX "Software\Classes\\{{protocol}}"
    ${EndIf}
  {{/each}}


  ; Delete uninstaller
  Delete "$INSTDIR\uninstall.exe"

  {{#each resources_ancestors}}
  RMDir /REBOOTOK "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  ; Remove shortcuts if not updating
  ${If} $UpdateMode <> 1
    !insertmacro DeleteAppUserModelId

    ; Remove start menu shortcut
    ; MEOWO: STARTMENU 页已移除（GETFOLDER 宏依赖页面声明）；未配置注册表存储时
    ; 该宏本就退化为取当前变量值，按配置直接回填等价。
    !if "${STARTMENUFOLDER}" != ""
      StrCpy $AppStartMenuFolder "${STARTMENUFOLDER}"
    !endif
    !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
      RMDir "$SMPROGRAMS\$AppStartMenuFolder"
    ${EndIf}
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Remove desktop shortcuts
    !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\${PRODUCTNAME}.lnk"
      Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}

  ; Remove registry information for add/remove programs
  !if "${INSTALLMODE}" == "both"
    DeleteRegKey SHCTX "${UNINSTKEY}"
  !else if "${INSTALLMODE}" == "perMachine"
    DeleteRegKey HKLM "${UNINSTKEY}"
  !else
    DeleteRegKey HKCU "${UNINSTKEY}"
  !endif

  ; Removes the Autostart entry for ${PRODUCTNAME} from the HKCU Run key if it exists.
  ; This ensures the program does not launch automatically after uninstallation if it exists.
  ; If it doesn't exist, it does nothing.
  ; We do this when not updating (to preserve the registry value on updates)
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
    ; MEOWO: 对话功能种子正常已被应用读后即焚，这里是「装完从未启动就卸载」的兜底。
    DeleteRegValue SHCTX "${MANUPRODUCTKEY}" "ChatEnabled"
  ${EndIf}

  ; Delete app data if the checkbox is selected
  ; and if not updating
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Clear the install location $INSTDIR from registry
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"

    ; Clear the install language from registry
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
    DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty HKCU "${MANUKEY}"

    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}"
    RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}

  !ifmacrodef NSIS_HOOK_POSTUNINSTALL
    !insertmacro NSIS_HOOK_POSTUNINSTALL
  !endif

  ; Auto close if passive mode or updating
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
    SetAutoClose true
  ${EndIf}
SectionEnd

Function RestorePreviousInstallLocation
  ReadRegStr $4 SHCTX "${MANUPRODUCTKEY}" ""
  StrCmp $4 "" +2 0
    StrCpy $INSTDIR $4
FunctionEnd

Function Skip
  Abort
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd
Function un.SkipIfPassive
  ${IfThen} $PassiveMode = 1  ${|} Abort ${|}
FunctionEnd

Function CreateOrUpdateStartMenuShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  StrCpy $R0 0

  !insertmacro IsShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    StrCpy $R0 1
  ${EndIf}

  ${If} $R0 = 1
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  !if "${STARTMENUFOLDER}" != ""
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
FunctionEnd

Function CreateOrUpdateDesktopShortcut
  ; We used to use product name as MAINBINARYNAME
  ; migrate old shortcuts to target the new MAINBINARYNAME
  !insertmacro IsShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\$OldMainBinaryName"
  Pop $0
  ${If} $0 = 1
    !insertmacro SetShortcutTarget "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Return
  ${EndIf}

  ; Skip creating shortcut if in update mode or no shortcut mode
  ; but always create if migrating from wix
  ${If} $WixMode = 0
    ${If} $UpdateMode = 1
    ${OrIf} $NoShortcutMode = 1
      Return
    ${EndIf}
  ${EndIf}

  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
FunctionEnd
