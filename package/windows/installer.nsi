; Wachiland Launcher installer (cargo-packager NSIS template).
;
; Derived from cargo-packager 0.11.8's default template. Differences:
; - per-user install in %LOCALAPPDATA%\Programs so the signed in-place updater
;   (crates/backend/src/launcher_install.rs) can replace the exe without UAC;
; - Spanish-only text, dark launcher palette, DPI-aware bitmaps;
; - upgrades overwrite in place (no uninstall/reinstall page) and compare the
;   real exe version, because the launcher's own updater does not rewrite the
;   uninstall registry key;
; - launcher data (%APPDATA%\WachilandLauncher) is never touched;
; - shortcuts carry no explicit AppUserModelID because the process sets none.

SetCompressor /SOLID lzma
Unicode true
ManifestDPIAware true
ManifestSupportedOS Win10

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh
!include WordFunc.nsh
!include LogicLib.nsh
!include WinMessages.nsh

!define MANUFACTURER "{{manufacturer}}"
!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define VERSIONWITHBUILD "{{version_with_build}}"
!define SHORTDESCRIPTION "{{short_description}}"
!define INSTALLERICON "{{installer_icon}}"
!define SIDEBARIMAGE "{{sidebar_image}}"
!define HEADERIMAGE "{{header_image}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define MAINBINARYSRCPATH "{{main_binary_path}}"
!define IDENTIFIER "{{identifier}}"
!define COPYRIGHT "{{copyright}}"
!define OUTFILE "{{out_file}}"
!define PLUGINSPATH "{{additional_plugins_path}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define MANUPRODUCTKEY "Software\${MANUFACTURER}\${PRODUCTNAME}"
!define UNINSTALLERSIGNCOMMAND "{{uninstaller_sign_cmd}}"
!define ESTIMATEDSIZE "{{estimated_size}}"

; Launcher dark palette (assets/theme/tokens.json). RRGGBB for SetCtlColors,
; 0xBBGGRR for Win32 COLORREF messages.
!define C_PANEL "0F0F12"
!define C_SUNKEN "0B0B0D"
!define C_INK "ECECEE"
!define C_MUTED "9A9AA3"
!define REF_PANEL 0x120F0F
!define REF_SUNKEN 0x0D0B0B
!define REF_INK 0xEEECEC
!define REF_ACCENT 0xF5CE5B
!define REF_TRACK 0x1A1616

Name "${PRODUCTNAME}"
; Windows draws the disabled branding label embossed, which looks smudged on
; the dark frame; the version is on the welcome page instead.
BrandingText " "
OutFile "${OUTFILE}"
RequestExecutionLevel user
ShowInstDetails hide
ShowUninstDetails hide

VIProductVersion "${VERSIONWITHBUILD}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "CompanyName" "${MANUFACTURER}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME} – instalador"
VIAddVersionKey "LegalCopyright" "${COPYRIGHT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "OriginalFilename" "WachilandLauncher-Setup.exe"

!if "${PLUGINSPATH}" != ""
  !addplugindir "${PLUGINSPATH}"
!endif

!if "${UNINSTALLERSIGNCOMMAND}" != ""
  !uninstfinalize '${UNINSTALLERSIGNCOMMAND}'
!endif

!if "${INSTALLERICON}" != ""
  !define MUI_ICON "${INSTALLERICON}"
  !define MUI_UNICON "${INSTALLERICON}"
!endif
!if "${SIDEBARIMAGE}" != ""
  !define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
  !define MUI_UNWELCOMEFINISHPAGE_BITMAP "${SIDEBARIMAGE}"
!endif
!if "${HEADERIMAGE}" != ""
  !define MUI_HEADERIMAGE
  !define MUI_HEADERIMAGE_RIGHT
  !define MUI_HEADERIMAGE_BITMAP "${HEADERIMAGE}"
  !define MUI_HEADERIMAGE_UNBITMAP "${HEADERIMAGE}"
!endif

!define MUI_BGCOLOR "${C_PANEL}"
!define MUI_TEXTCOLOR "${C_INK}"
!define MUI_ABORTWARNING
!define MUI_UNABORTWARNING
!define MUI_CUSTOMFUNCTION_GUIINIT DarkFrame
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.DarkFrame

; Per-scale bitmap variants live next to the fallback sidebar image
; (package/windows/generate_installer_art.py).
!searchreplace ARTDIR "${SIDEBARIMAGE}" "installer-sidebar.bmp" ""
!macro ART_FILES PREFIX NAME
  !insertmacro ART_FILE ${PREFIX} ${NAME} 100
  !insertmacro ART_FILE ${PREFIX} ${NAME} 125
  !insertmacro ART_FILE ${PREFIX} ${NAME} 150
  !insertmacro ART_FILE ${PREFIX} ${NAME} 175
  !insertmacro ART_FILE ${PREFIX} ${NAME} 200
!macroend
!macro ART_FILE PREFIX NAME SCALE
  File "/oname=$PLUGINSDIR\${PREFIX}-${SCALE}.bmp" "${ARTDIR}installer-${NAME}-${SCALE}.bmp"
!macroend

; ---------------------------------------------------------------------------
; Dark styling helpers, defined for both the installer and the uninstaller.
!macro DARK_FUNCTIONS UN
Function ${UN}DarkControl
  ; $R8 = control. Classic theme for check/radio boxes so their text colour
  ; applies; the Windows dark theme for push buttons, edits and scroll bars.
  System::Call 'user32::GetClassNameW(p $R8, w .R7, i 64)'
  ${If} $R7 == "Button"
    System::Call 'user32::GetWindowLongW(p $R8, i -16) i .R6'
    IntOp $R6 $R6 & 0xF
    ${If} $R6 >= 2
    ${AndIf} $R6 != 8
    ${AndIf} $R6 != 11
      System::Call 'uxtheme::SetWindowTheme(p $R8, w " ", w " ")'
      SetCtlColors $R8 ${C_INK} ${C_PANEL}
    ${Else}
      System::Call 'uxtheme::SetWindowTheme(p $R8, w "DarkMode_Explorer", p 0)'
    ${EndIf}
  ${ElseIf} $R7 == "Edit"
    System::Call 'uxtheme::SetWindowTheme(p $R8, w "DarkMode_CFD", p 0)'
    SetCtlColors $R8 ${C_INK} ${C_SUNKEN}
  ${ElseIf} $R7 == "msctls_progress32"
    System::Call 'uxtheme::SetWindowTheme(p $R8, w " ", w " ")'
    SendMessage $R8 0x0409 0 ${REF_ACCENT}
    SendMessage $R8 0x2001 0 ${REF_TRACK}
  ${ElseIf} $R7 == "SysListView32"
    System::Call 'uxtheme::SetWindowTheme(p $R8, w "DarkMode_Explorer", p 0)'
    SendMessage $R8 0x1001 0 ${REF_SUNKEN}
    SendMessage $R8 0x1026 0 ${REF_SUNKEN}
    SendMessage $R8 0x1024 0 ${REF_INK}
  ${Else}
    SetCtlColors $R8 ${C_INK} ${C_PANEL}
  ${EndIf}
FunctionEnd

Function ${UN}DarkChildren
  ; $R9 = parent window
  Push $R8
  StrCpy $R8 0
  ${Do}
    FindWindow $R8 "" "" $R9 $R8
    ${If} $R8 == 0
      ${Break}
    ${EndIf}
    Call ${UN}DarkControl
  ${Loop}
  Pop $R8
FunctionEnd

Function ${UN}DarkFrame
  ; Dark title bar (Windows 10 2004+ attribute 20, older builds 19).
  System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i 1, i 4) i .R0'
  ${If} $R0 != 0
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 19, *i 1, i 4)'
  ${EndIf}
  SetCtlColors $HWNDPARENT ${C_INK} ${C_PANEL}
  StrCpy $R9 $HWNDPARENT
  Call ${UN}DarkChildren
  GetDlgItem $R8 $HWNDPARENT 1028
  SetCtlColors $R8 ${C_MUTED} ${C_PANEL}
  ; 1256 is the light "etched" twin of the branding text.
  GetDlgItem $R8 $HWNDPARENT 1256
  ShowWindow $R8 ${SW_HIDE}
  ; Header texts keep the MUI colours but the title is drawn in the accent ink.
  GetDlgItem $R8 $HWNDPARENT 1037
  SetCtlColors $R8 ${C_INK} ${C_PANEL}
  GetDlgItem $R8 $HWNDPARENT 1038
  SetCtlColors $R8 ${C_MUTED} ${C_PANEL}
  ; Header bitmap at the exact size of this display scale.
  GetDlgItem $R8 $HWNDPARENT 1046
  StrCpy $R7 "head"
  StrCpy $R6 175
  Call ${UN}ArtVariant
  SetBrandingImage /IMGID=1046 /RESIZETOFIT $R5
FunctionEnd

Function ${UN}ArtVariant
  ; $R8 = image control, $R7 = variant prefix, $R6 = its width at 100 %.
  ; Returns in $R5 the extracted bitmap closest to the control's scale.
  System::Call '*(i, i, i, i) p .R4'
  System::Call 'user32::GetClientRect(p $R8, p $R4)'
  System::Call '*$R4(i, i, i .R3, i)'
  System::Free $R4
  IntOp $R3 $R3 * 100
  IntOp $R3 $R3 / $R6
  ${If} $R3 <= 112
    StrCpy $R5 100
  ${ElseIf} $R3 <= 137
    StrCpy $R5 125
  ${ElseIf} $R3 <= 162
    StrCpy $R5 150
  ${ElseIf} $R3 <= 187
    StrCpy $R5 175
  ${Else}
    StrCpy $R5 200
  ${EndIf}
  StrCpy $R5 "$PLUGINSDIR\$R7-$R5.bmp"
FunctionEnd

Function ${UN}ExtractArt
  InitPluginsDir
  !insertmacro ART_FILES head header
FunctionEnd

Function ${UN}DarkPage
  ; MUI re-shows the etched separators per page; on the dark frame they read as
  ; light seams.
  GetDlgItem $R8 $HWNDPARENT 1035
  ShowWindow $R8 ${SW_HIDE}
  GetDlgItem $R8 $HWNDPARENT 1036
  ShowWindow $R8 ${SW_HIDE}
  GetDlgItem $R8 $HWNDPARENT 1045
  ShowWindow $R8 ${SW_HIDE}
  FindWindow $R9 "#32770" "" $HWNDPARENT
  SetCtlColors $R9 ${C_INK} ${C_PANEL}
  Call ${UN}DarkChildren
FunctionEnd
!macroend
!insertmacro DARK_FUNCTIONS ""
!insertmacro DARK_FUNCTIONS "un."

; ---------------------------------------------------------------------------
; Installer pages
Var Upgrading
Var PassiveMode
; Migration from another launcher's folder (see "Migration" below).
Var MigrateWanted
Var MigrateDir
Var MigrateVoxy
Var MigrateResult
Var MigrateCheck
Var MigrateField
Var MigrateBrowse
Var MigrateVoxyCheck

; The global profile that a migrated folder becomes an instance of.
!define MIGRATION_PROFILE "profile_wachiland-elite"
!define MIGRATION_NAME "Wachiland Elite"

!define MUI_WELCOMEPAGE_TITLE "$(welcomeTitle)"
!define MUI_WELCOMEPAGE_TEXT "$(welcomeText)"
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW WelcomeShow
!insertmacro MUI_PAGE_WELCOME

!define MUI_PAGE_HEADER_TEXT "$(directoryTitle)"
!define MUI_PAGE_HEADER_SUBTEXT "$(directorySubtitle)"
!define MUI_DIRECTORYPAGE_TEXT_TOP "$(directoryText)"
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "$(directoryBox)"
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipDirectory
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DarkPage
!insertmacro MUI_PAGE_DIRECTORY

Page custom MigrationShow MigrationLeave

!define MUI_PAGE_HEADER_TEXT "$(installingTitle)"
!define MUI_PAGE_HEADER_SUBTEXT "$(installingSubtitle)"
!define MUI_INSTFILESPAGE_FINISHHEADER_TEXT "$(installedTitle)"
!define MUI_INSTFILESPAGE_FINISHHEADER_SUBTEXT "$(installedSubtitle)"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DarkPage
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_TITLE "$(finishTitle)"
!define MUI_FINISHPAGE_TEXT "$(finishText)"
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "$(runApp)"
!define MUI_FINISHPAGE_RUN_FUNCTION RunApp
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "$(createDesktop)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipIfPassive
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishShow
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!define MUI_PAGE_HEADER_TEXT "$(uninstallTitle)"
!define MUI_PAGE_HEADER_SUBTEXT "$(uninstallSubtitle)"
!define MUI_UNCONFIRMPAGE_TEXT_TOP "$(uninstallText)"
!define MUI_UNCONFIRMPAGE_TEXT_LOCATION "$(uninstallFrom)"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.DarkPage
!insertmacro MUI_UNPAGE_CONFIRM
!define MUI_PAGE_HEADER_TEXT "$(uninstallingTitle)"
!define MUI_PAGE_HEADER_SUBTEXT "$(uninstallingSubtitle)"
!define MUI_INSTFILESPAGE_FINISHHEADER_TEXT "$(uninstalledTitle)"
!define MUI_INSTFILESPAGE_FINISHHEADER_SUBTEXT "$(uninstalledSubtitle)"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.DarkPage
!insertmacro MUI_UNPAGE_INSTFILES

{{#each languages}}
!insertmacro MUI_LANGUAGE "{{this}}"
{{/each}}
{{#each language_files}}
!include "{{this}}"
{{/each}}

; ---------------------------------------------------------------------------
Function .onInit
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  Call ExtractArt
  !insertmacro ART_FILES side sidebar

  ; /MIGRATE=<folder> [/MIGRATEVOXY] migrate without the page (silent installs).
  ${GetOptions} $CMDLINE "/MIGRATE=" $R0
  ${IfNot} ${Errors}
  ${AndIf} $R0 != ""
    StrCpy $MigrateDir $R0
    StrCpy $MigrateWanted 1
  ${EndIf}
  ${GetOptions} $CMDLINE "/MIGRATEVOXY" $R0
  ${IfNot} ${Errors}
    StrCpy $MigrateVoxy 1
  ${EndIf}

  StrCpy $Upgrading 0
  ReadRegStr $R0 HKCU "${MANUPRODUCTKEY}" ""
  ${If} $R0 != ""
  ${AndIf} ${FileExists} "$R0\${MAINBINARYNAME}.exe"
    StrCpy $INSTDIR $R0
    StrCpy $Upgrading 1
    ; The launcher updates itself in place, so the registry DisplayVersion can
    ; be stale: compare against the installed executable instead.
    ClearErrors
    GetDLLVersion "$R0\${MAINBINARYNAME}.exe" $R1 $R2
    ${IfNot} ${Errors}
      IntOp $R3 $R1 >> 16
      IntOp $R4 $R1 & 0xFFFF
      IntOp $R5 $R2 >> 16
      StrCpy $R6 "$R3.$R4.$R5"
      ${VersionCompare} "${VERSION}" $R6 $R7
      ${If} $R7 == 2
        ${If} ${Silent}
          Abort
        ${EndIf}
        MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "$(newerInstalled)" IDYES newer_accepted
        Abort
        newer_accepted:
      ${EndIf}
    ${EndIf}
  ${ElseIf} $INSTDIR == ""
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
  ${EndIf}
FunctionEnd

Function SkipIfPassive
  ${IfThen} $PassiveMode == 1 ${|} Abort ${|}
FunctionEnd

Function SkipDirectory
  ${IfThen} $PassiveMode == 1 ${|} Abort ${|}
  ${IfThen} $Upgrading == 1 ${|} Abort ${|}
FunctionEnd

!macro EXACT_SIDEBAR CONTROL BITMAP
  StrCpy $R8 ${CONTROL}
  StrCpy $R7 "side"
  StrCpy $R6 191
  Call ArtVariant
  ${NSD_FreeImage} ${BITMAP}
  ${NSD_SetStretchedImage} ${CONTROL} $R5 ${BITMAP}
!macroend

Function WelcomeShow
  Call DarkPage
  ; Welcome/finish pages are their own nsDialogs window, not the inner page.
  StrCpy $R9 $mui.WelcomePage
  SetCtlColors $R9 ${C_INK} ${C_PANEL}
  Call DarkChildren
  !insertmacro EXACT_SIDEBAR $mui.WelcomePage.Image $mui.WelcomePage.Image.Bitmap
FunctionEnd

Function FinishShow
  Call DarkPage
  StrCpy $R9 $mui.FinishPage
  SetCtlColors $R9 ${C_INK} ${C_PANEL}
  Call DarkChildren
  ; Nothing to go back to once the files are installed.
  GetDlgItem $R8 $HWNDPARENT 3
  ShowWindow $R8 ${SW_HIDE}
  !insertmacro EXACT_SIDEBAR $mui.FinishPage.Image $mui.FinishPage.Image.Bitmap
  ; On an upgrade keep the user's earlier choice about the desktop shortcut.
  ${If} $Upgrading == 1
  ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
    SendMessage $mui.FinishPage.ShowReadme ${BM_SETCHECK} ${BST_UNCHECKED} 0
  ${EndIf}
  ${If} $MigrateResult == "ok"
    ${NSD_SetText} $mui.FinishPage.Text "$(finishTextMigrated)"
  ${ElseIf} $MigrateResult == "failed"
    ${NSD_SetText} $mui.FinishPage.Text "$(finishTextMigrationFailed)"
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------------------
; Migration: copy the player's Wachiland Elite folder from another launcher
; into a new instance of the global profile. The launcher does the work
; (--migrate-legacy): it verifies the signed profile, reuses the pack files the
; player already has, creates the instance like the GUI and then copies only
; player data. The old folder is only read.
Function FindLegacyFolder
  ${If} $MigrateDir != ""
    Return
  ${EndIf}
  ${If} ${FileExists} "$APPDATA\.minecraft_wachilandelite\automodpack\modpacks\*Wachiland*"
    StrCpy $MigrateDir "$APPDATA\.minecraft_wachilandelite"
    Return
  ${EndIf}
  ; Players' folders have different names: take the first one in AppData with
  ; an AutoModpack Wachiland modpack.
  FindFirst $R0 $R1 "$APPDATA\*"
  ${DoWhile} $R1 != ""
    ${If} $R1 != "."
    ${AndIf} $R1 != ".."
    ${AndIf} ${FileExists} "$APPDATA\$R1\automodpack\modpacks\*Wachiland*"
      StrCpy $MigrateDir "$APPDATA\$R1"
      ${Break}
    ${EndIf}
    FindNext $R0 $R1
  ${Loop}
  FindClose $R0
FunctionEnd

Function MigrationShow
  ${IfThen} $PassiveMode == 1 ${|} Abort ${|}
  !insertmacro MUI_HEADER_TEXT "$(migrateTitle)" "$(migrateSubtitle)"
  Call FindLegacyFolder
  nsDialogs::Create 1018
  Pop $R5
  ${If} $R5 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 44u "$(migrateText)"
  Pop $R0
  ${NSD_CreateCheckbox} 0 48u 100% 12u "$(migrateCheck)"
  Pop $MigrateCheck
  ${NSD_OnClick} $MigrateCheck MigrationClick
  ${NSD_CreateLabel} 12u 64u 88% 10u "$(migrateFolder)"
  Pop $R0
  ${NSD_CreateText} 12u 76u 66% 13u "$MigrateDir"
  Pop $MigrateField
  ${NSD_CreateButton} 80% 75u 20% 15u "$(migrateBrowse)"
  Pop $MigrateBrowse
  ${NSD_OnClick} $MigrateBrowse MigrationBrowse
  ${NSD_CreateCheckbox} 12u 94u 88% 12u "$(migrateVoxy)"
  Pop $MigrateVoxyCheck
  ${NSD_CreateLabel} 0 112u 100% 26u "$(migrateWarning)"
  Pop $R0
  ${If} $MigrateWanted == 1
    ${NSD_Check} $MigrateCheck
  ${EndIf}
  ${If} $MigrateVoxy == 1
    ${NSD_Check} $MigrateVoxyCheck
  ${EndIf}
  Call DarkPage
  StrCpy $R9 $R5
  SetCtlColors $R9 ${C_INK} ${C_PANEL}
  Call DarkChildren
  ; The warning in the accent colour, like the launcher's notices.
  SetCtlColors $R0 F5CE5B ${C_PANEL}
  Call MigrationEnable
  nsDialogs::Show
FunctionEnd

Function MigrationClick
  Pop $R0
  Call MigrationEnable
FunctionEnd

Function MigrationEnable
  ${NSD_GetState} $MigrateCheck $R1
  ${If} $R1 == ${BST_CHECKED}
    StrCpy $R1 1
  ${Else}
    StrCpy $R1 0
  ${EndIf}
  EnableWindow $MigrateField $R1
  EnableWindow $MigrateBrowse $R1
  EnableWindow $MigrateVoxyCheck $R1
FunctionEnd

Function MigrationBrowse
  Pop $R0
  ; Always opens in AppData, where the old launchers keep their folders.
  nsDialogs::SelectFolderDialog "$(migrateBrowseTitle)" "$APPDATA"
  Pop $R0
  ${If} $R0 != "error"
  ${AndIf} $R0 != ""
    ${NSD_SetText} $MigrateField $R0
  ${EndIf}
FunctionEnd

Function MigrationLeave
  ${NSD_GetText} $MigrateField $MigrateDir
  ; A trailing backslash would escape the closing quote on the command line.
  ${Do}
    StrCpy $R0 $MigrateDir 1 -1
    ${If} $R0 != "\"
      ${Break}
    ${EndIf}
    StrCpy $MigrateDir $MigrateDir -1
  ${Loop}
  StrCpy $MigrateWanted 0
  ${NSD_GetState} $MigrateCheck $R0
  ${If} $R0 == ${BST_CHECKED}
    ${IfNot} ${FileExists} "$MigrateDir\options.txt"
    ${AndIfNot} ${FileExists} "$MigrateDir\saves\*.*"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(migrateNotFound)"
      Abort
    ${EndIf}
    StrCpy $MigrateWanted 1
  ${EndIf}
  StrCpy $MigrateVoxy 0
  ${NSD_GetState} $MigrateVoxyCheck $R0
  ${IfThen} $R0 == ${BST_CHECKED} ${|} StrCpy $MigrateVoxy 1 ${|}
FunctionEnd

!macro CheckIfAppIsRunning
  nsis_tauri_utils::FindProcess "${MAINBINARYNAME}.exe"
  Pop $R0
  ${If} $R0 = 0
    IfSilent kill 0
    ${IfThen} $PassiveMode != 1 ${|} MessageBox MB_OKCANCEL|MB_ICONINFORMATION "$(appRunningOkKill)" IDOK kill IDCANCEL cancel ${|}
    kill:
      nsis_tauri_utils::KillProcess "${MAINBINARYNAME}.exe"
      Pop $R0
      Sleep 800
      ${If} $R0 = 0
        Goto app_check_done
      ${EndIf}
      Abort "$(failedToKillApp)"
    cancel:
      Abort "$(appRunning)"
  ${EndIf}
  app_check_done:
!macroend

{{#if preinstall_section}}
{{unescape_newlines preinstall_section}}
{{/if}}

Section Install
  SetOutPath $INSTDIR
  !insertmacro CheckIfAppIsRunning

  File "${MAINBINARYSRCPATH}"
  {{#each resources_dirs}}
  CreateDirectory "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each resources}}
  File /a "/oname={{this}}" "{{@key}}"
  {{/each}}
  {{#each binaries}}
  File /a "/oname={{this}}" "{{@key}}"
  {{/each}}

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "" $INSTDIR

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$\"$INSTDIR$\""
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" "1"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" "1"
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "${ESTIMATEDSIZE}"

  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\${MAINBINARYNAME}.exe" 0 SW_SHOWNORMAL "" "${SHORTDESCRIPTION}"

  ; Silent/passive installs create both shortcuts unless /NS is passed.
  IfSilent check_ns 0
  ${IfThen} $PassiveMode == 1 ${|} Goto check_ns ${|}
  Goto shortcuts_done
  check_ns:
    ${GetOptions} $CMDLINE "/NS" $R0
    IfErrors 0 shortcuts_done
      Call CreateDesktopShortcut
  shortcuts_done:
  ${IfThen} $PassiveMode == 1 ${|} SetAutoClose true ${|}
SectionEnd

Section Migrate
  ${If} $MigrateWanted != 1
    Return
  ${EndIf}
  SetDetailsView show
  DetailPrint "$(migrateRunning)"
  StrCpy $R1 ""
  ${IfThen} $MigrateVoxy == 1 ${|} StrCpy $R1 " --migrate-voxy" ${|}
  nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --migrate-legacy "$MigrateDir" --from-profile ${MIGRATION_PROFILE} --migrate-name "${MIGRATION_NAME}"$R1'
  Pop $R0
  ${If} $R0 == 0
    StrCpy $MigrateResult "ok"
    DetailPrint "$(migrateDone)"
  ${Else}
    StrCpy $MigrateResult "failed"
    DetailPrint "$(migrateFailedLog) ($R0)"
    ${IfNot} ${Silent}
    ${AndIf} $PassiveMode != 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(migrateFailed)"
    ${EndIf}
  ${EndIf}
SectionEnd

Function .onInstSuccess
  IfSilent check_r 0
  ${IfThen} $PassiveMode == 1 ${|} Goto check_r ${|}
  Goto run_done
  check_r:
    ${GetOptions} $CMDLINE "/R" $R0
    IfErrors run_done 0
      Call RunApp
  run_done:
FunctionEnd

Function RunApp
  ; Start from the install folder like a shortcut would.
  SetOutPath $INSTDIR
  Exec '"$INSTDIR\${MAINBINARYNAME}.exe"'
FunctionEnd

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\${MAINBINARYNAME}.exe" 0 SW_SHOWNORMAL "" "${SHORTDESCRIPTION}"
FunctionEnd

; ---------------------------------------------------------------------------
Function un.onInit
  SetShellVarContext current
  SetRegView 64
  Call un.ExtractArt
  ${GetOptions} $CMDLINE "/P" $PassiveMode
  ${IfNot} ${Errors}
    StrCpy $PassiveMode 1
  ${EndIf}
FunctionEnd

Section Uninstall
  !insertmacro CheckIfAppIsRunning

  Delete "$INSTDIR\${MAINBINARYNAME}.exe"
  ; Recovery copies and interrupted downloads left by the in-place updater.
  Delete "$INSTDIR\${MAINBINARYNAME}.exe.wbombo-previous-*"
  Delete "$INSTDIR\${MAINBINARYNAME}.exe.wbombo-new-*"
  {{#each resources}}
  Delete "$INSTDIR\\{{this}}"
  {{/each}}
  {{#each binaries}}
  Delete "$INSTDIR\\{{this}}"
  {{/each}}
  Delete "$INSTDIR\uninstall.exe"
  {{#each resources_dirs}}
  RMDir "$INSTDIR\\{{this}}"
  {{/each}}
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"

  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${MANUPRODUCTKEY}"
  DeleteRegKey /ifempty HKCU "Software\${MANUFACTURER}"

  ${IfThen} $PassiveMode == 1 ${|} SetAutoClose true ${|}
SectionEnd
