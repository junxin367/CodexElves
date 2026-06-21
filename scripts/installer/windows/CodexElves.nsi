Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!define ROOT "..\..\.."

Name "CodexElves"
OutFile "${ROOT}\dist\windows\CodexElves-${VERSION}-windows-x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\CodexElves"
InstallDirRegKey HKCU "Software\CodexElves" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!define MUI_ICON "${ROOT}\apps\codex-elves-manager\src-tauri\icons\icon.ico"
!define MUI_UNICON "${ROOT}\apps\codex-elves-manager\src-tauri\icons\icon.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"

  nsExec::ExecToLog 'taskkill /IM codex-elves.exe /F'
  Pop $0
  nsExec::ExecToLog 'taskkill /IM codex-elves-manager.exe /F'
  Pop $0

  File "${ROOT}\dist\windows\app\codex-elves.exe"
  File "${ROOT}\dist\windows\app\codex-elves-manager.exe"

  Delete "$DESKTOP\CodexElves 绠＄悊宸ュ叿.lnk"
  Delete "$SMPROGRAMS\CodexElves\CodexElves 绠＄悊宸ュ叿.lnk"
  Delete "$SMPROGRAMS\CodexElves\鍗歌浇 CodexElves.lnk"

  CreateShortcut "$DESKTOP\CodexElves.lnk" "$INSTDIR\codex-elves.exe" "" "$INSTDIR\codex-elves.exe"
  CreateShortcut "$DESKTOP\CodexElves 管理工具.lnk" "$INSTDIR\codex-elves-manager.exe" "" "$INSTDIR\codex-elves-manager.exe"
  CreateShortcut "$DESKTOP\CodexElves Manager.lnk" "$INSTDIR\codex-elves-manager.exe" "" "$INSTDIR\codex-elves-manager.exe"
  CreateDirectory "$SMPROGRAMS\CodexElves"
  CreateShortcut "$SMPROGRAMS\CodexElves\CodexElves.lnk" "$INSTDIR\codex-elves.exe" "" "$INSTDIR\codex-elves.exe"
  CreateShortcut "$SMPROGRAMS\CodexElves\CodexElves 管理工具.lnk" "$INSTDIR\codex-elves-manager.exe" "" "$INSTDIR\codex-elves-manager.exe"
  CreateShortcut "$SMPROGRAMS\CodexElves\卸载 CodexElves.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\codex-elves-manager.exe"
  CreateShortcut "$SMPROGRAMS\CodexElves Manager.lnk" "$INSTDIR\codex-elves-manager.exe" "" "$INSTDIR\codex-elves-manager.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\CodexElves" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "DisplayName" "CodexElves"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "Publisher" "junxin367"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "DisplayIcon" "$INSTDIR\codex-elves-manager.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves" "UninstallString" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  nsExec::ExecToLog 'taskkill /IM codex-elves.exe /F'
  Pop $0
  nsExec::ExecToLog 'taskkill /IM codex-elves-manager.exe /F'
  Pop $0

  Delete "$DESKTOP\CodexElves.lnk"
  Delete "$DESKTOP\CodexElves 管理工具.lnk"
  Delete "$DESKTOP\CodexElves Manager.lnk"
  Delete "$DESKTOP\CodexElves 绠＄悊宸ュ叿.lnk"
  Delete "$SMPROGRAMS\CodexElves\CodexElves.lnk"
  Delete "$SMPROGRAMS\CodexElves\CodexElves 管理工具.lnk"
  Delete "$SMPROGRAMS\CodexElves\CodexElves 绠＄悊宸ュ叿.lnk"
  Delete "$SMPROGRAMS\CodexElves\卸载 CodexElves.lnk"
  Delete "$SMPROGRAMS\CodexElves\鍗歌浇 CodexElves.lnk"
  Delete "$SMPROGRAMS\CodexElves Manager.lnk"
  RMDir "$SMPROGRAMS\CodexElves"

  Delete "$INSTDIR\codex-elves.exe"
  Delete "$INSTDIR\codex-elves-manager.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexElves"
  DeleteRegKey HKCU "Software\CodexElves"
SectionEnd
