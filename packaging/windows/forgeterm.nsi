; ForgeTerm installer for Windows (NSIS 3, Unicode). Built by scripts\package-windows.ps1.
;
; Per user, no administrator rights: installs to %LOCALAPPDATA%\Programs\ForgeTerm,
; adds a Start menu shortcut (desktop shortcut optional) and an entry under
; Settings > Apps. The uninstaller removes exactly the files it installed and keeps
; the user's settings and saved session in %APPDATA%\ForgeTerm.
;
; Defines passed by the build script: VERSION (x.y.z), STAGE (folder with the files),
; ICON (forgeterm.ico), SIDEBAR (welcome/finish image, scripts\installer-art.ps1), OUTFILE.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!define APP "ForgeTerm"
!define PUBLISHER "Erik van de Blaak"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"

Name "${APP} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
BrandingText "${APP} ${VERSION} — Created by ${PUBLISHER}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "© ${PUBLISHER}"
VIAddVersionKey "FileDescription" "${APP} installer"

!include "MUI2.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${SIDEBAR}"
!define MUI_WELCOMEFINISHPAGE_BITMAP_STRETCH AspectFitHeight
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "This installs ${APP} ${VERSION} for your Windows account only. No administrator rights are needed.$\r$\n$\r$\nYour settings in %APPDATA%\${APP} are kept, also when you uninstall."
!define MUI_FINISHPAGE_RUN "$INSTDIR\forgeterm.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"
!define MUI_FINISHPAGE_RUN_NOTCHECKED

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Waits until forgeterm.exe in $INSTDIR is not running (it cannot be replaced or
; removed while it runs). Opening the file for appending fails while it is in use.
!macro WaitUntilClosed
  ${If} ${FileExists} "$INSTDIR\forgeterm.exe"
    ${Do}
      ClearErrors
      FileOpen $0 "$INSTDIR\forgeterm.exe" a
      ${IfNot} ${Errors}
        FileClose $0
        ${Break}
      ${EndIf}
      ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${APP} is still running. Close all ${APP} windows and choose Retry." /SD IDCANCEL IDCANCEL`
        Abort
      ${EndIf}
    ${Loop}
  ${EndIf}
!macroend

Section "${APP}" SecMain
  SectionIn RO
  !insertmacro WaitUntilClosed
  SetOutPath "$INSTDIR"
  File "${STAGE}\forgeterm.exe"
  File "${STAGE}\README.md"
  File "${STAGE}\LICENSE-MIT"
  File "${STAGE}\LICENSE-APACHE"
  File "${STAGE}\THIRD-PARTY.txt"
  File "${STAGE}\JetBrainsMono-OFL.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\forgeterm.exe" "" "$INSTDIR\forgeterm.exe" 0

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\forgeterm.exe,0"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  SectionGetSize ${SecMain} $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" $0
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\forgeterm.exe" "" "$INSTDIR\forgeterm.exe" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "${APP} with a Start menu shortcut."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Also put a shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro WaitUntilClosed
  ; Only the files this installer wrote; RMDir without /r leaves anything else alone.
  Delete "$INSTDIR\forgeterm.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE-MIT"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\THIRD-PARTY.txt"
  Delete "$INSTDIR\JetBrainsMono-OFL.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
SectionEnd
