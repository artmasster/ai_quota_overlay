; Ai Quota Overlay — Windows installer (per user, no administrator prompt).
;
;   makensis -DVERSION=2.0.0 -DEXE_PATH=<ai-quota-overlay.exe> -DOUTFILE=<setup.exe> installer\installer.nsi
;
; It installs where the Electron builds lived (%LOCALAPPDATA%\Programs\Ai Quota Overlay), under the same exe name,
; uninstaller name and registry keys electron-builder used (its app GUID for appId
; io.github.artmasster.ai-quota-overlay), so the Electron app's own updater can hand over to this installer:
; it runs it as `setup.exe /S --updated --force-run`. Any Electron files in the folder are removed first.
; Settings in %APPDATA%\Ai Quota Overlay are never touched.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "pass -DVERSION=x.y.z"
!endif
!ifndef EXE_PATH
  !error "pass -DEXE_PATH=<path to ai-quota-overlay.exe>"
!endif
!ifndef OUTFILE
  !define OUTFILE "AiQuotaOverlay-${VERSION}-win-x64-setup.exe"
!endif

; Signed releases (scripts/release-windows.ps1) sign the uninstaller as makensis writes it (NSIS 3.08+).
!ifdef SIGN_UNINSTALLER
  !uninstfinalize '${SIGN_UNINSTALLER} "%1"' = 0
!endif

!define PRODUCT "Ai Quota Overlay"
!define EXE "Ai Quota Overlay.exe"
!define UNINST "Uninstall Ai Quota Overlay.exe"
!define APP_GUID "17e5135a-090c-5bed-9a25-8486838d8a86"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_GUID}"
!define INSTALL_KEY "Software\${APP_GUID}"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT}"
InstallDirRegKey HKCU "${INSTALL_KEY}" "InstallLocation"
BrandingText "${PRODUCT} ${VERSION}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=0 "ProductName" "${PRODUCT}"
VIAddVersionKey /LANG=0 "FileDescription" "${PRODUCT} Setup"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "CompanyName" "artmasster"
VIAddVersionKey /LANG=0 "LegalCopyright" "MIT License"

!define MUI_ICON "../build/icon.ico"
!define MUI_UNICON "../build/icon.ico"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApp
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Thai"

LangString AppRunning ${LANG_ENGLISH} "${PRODUCT} is running. Close it and click Retry, or Cancel to quit setup."
LangString AppRunning ${LANG_THAI} "${PRODUCT} ยังเปิดอยู่ ปิดแอปแล้วกด Retry หรือกด Cancel เพื่อออก"

Var Updated
Var ForceRun

Function .onInit
  StrCpy $Updated 0
  StrCpy $ForceRun 0
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $Updated 1
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "--force-run" $R1
  ${IfNot} ${Errors}
    StrCpy $ForceRun 1
  ${EndIf}
FunctionEnd

; A running copy keeps its exe locked. An update waits for the app that started it to exit (and ends a stuck one
; after a while); an interactive install asks first.
!macro WaitForApp
  StrCpy $R2 0
  ${Do}
    ${IfNot} ${FileExists} "$INSTDIR\${EXE}"
      ${Break}
    ${EndIf}
    ClearErrors
    FileOpen $R3 "$INSTDIR\${EXE}" a
    ${IfNot} ${Errors}
      FileClose $R3
      ${Break}
    ${EndIf}
    IntOp $R2 $R2 + 1
    ${If} $R2 == 20
      ${If} ${Silent}
        ReadEnvStr $R5 USERNAME
        nsExec::Exec 'taskkill /F /IM "${EXE}" /FI "USERNAME eq $R5"'
        Pop $R4
      ${Else}
        MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(AppRunning)" /SD IDCANCEL IDRETRY +2
        Abort
        StrCpy $R2 0
      ${EndIf}
    ${ElseIf} $R2 >= 60
      Abort
    ${EndIf}
    Sleep 500
  ${Loop}
!macroend

; What an Electron build (electron-builder NSIS) left in the install folder.
Function RemoveElectron
  ${IfNot} ${FileExists} "$INSTDIR\resources\app.asar"
  ${AndIfNot} ${FileExists} "$INSTDIR\resources.pak"
    Return
  ${EndIf}
  RMDir /r "$INSTDIR\resources"
  RMDir /r "$INSTDIR\locales"
  RMDir /r "$INSTDIR\swiftshader"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\*.pak"
  Delete "$INSTDIR\*.bin"
  Delete "$INSTDIR\*.dat"
  Delete "$INSTDIR\*.json"
  Delete "$INSTDIR\LICENSE.electron.txt"
  Delete "$INSTDIR\LICENSES.chromium.html"
  Delete "$INSTDIR\version"
  Delete "$INSTDIR\${UNINST}"
  ; electron-updater's download cache
  RMDir /r "$LOCALAPPDATA\ai-quota-overlay-updater"
FunctionEnd

Section "Ai Quota Overlay" Main
  SetOutPath "$INSTDIR"
  !insertmacro WaitForApp
  Call RemoveElectron
  File "/oname=${EXE}" "${EXE_PATH}"
  WriteUninstaller "$INSTDIR\${UNINST}"

  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}"
  ; An update keeps the user's choice about the desktop shortcut.
  ${If} $Updated == 0
    CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE}"
  ${EndIf}

  WriteRegStr HKCU "${INSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${PRODUCT} ${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "artmasster"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE},0"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\${UNINST}" /currentuser'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINST}" /currentuser /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $R4 $R5 $R6
  IntFmt $R4 "0x%08X" $R4
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$R4"

  ; Silent installs start the app when asked (--force-run), as electron-builder's did; updates always do.
  ${If} ${Silent}
    ${If} $ForceRun == 1
    ${OrIf} $Updated == 1
      Call LaunchApp
    ${EndIf}
  ${EndIf}
SectionEnd

Function LaunchApp
  Exec '"$INSTDIR\${EXE}"'
FunctionEnd

Section "Uninstall"
  !insertmacro WaitForApp
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\${UNINST}"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  Delete "$DESKTOP\${PRODUCT}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "${INSTALL_KEY}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT}"
  RMDir /r "$LOCALAPPDATA\ai-quota-overlay-updater"
SectionEnd
