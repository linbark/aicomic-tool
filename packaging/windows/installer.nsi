Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!ifndef APP_VERSION
  !error "Pass /DAPP_VERSION=x.y.z"
!endif
!ifndef BINARY
  !error "Pass /DBINARY=path-to-aicomic-tool.exe"
!endif
!ifndef OUTPUT
  !error "Pass /DOUTPUT=installer-output-path"
!endif

Name "星晴"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Xingqing"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "星晴"
VIAddVersionKey /LANG=1033 "FileDescription" "星晴原生桌面安装器"
VIAddVersionKey /LANG=1033 "FileVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${APP_VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "星晴"
!define MUI_ICON "${__FILEDIR__}\..\icons\icon.ico"
!define MUI_UNICON "${__FILEDIR__}\..\icons\icon.ico"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "星晴需要 64 位 Windows。 / 64-bit Windows is required."
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "Software\Xingqing" "InstallDir"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

Section "星晴" Main
  SetOutPath "$INSTDIR"
  File /oname=aicomic-tool.exe "${BINARY}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\星晴"
  CreateShortcut "$SMPROGRAMS\星晴\星晴.lnk" "$INSTDIR\aicomic-tool.exe"
  CreateShortcut "$SMPROGRAMS\星晴\卸载星晴.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Xingqing" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "DisplayName" "星晴"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "Publisher" "星晴"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "DisplayIcon" "$INSTDIR\aicomic-tool.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing" "NoRepair" 1
SectionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext current
FunctionEnd
Section "Uninstall"
  Delete "$INSTDIR\aicomic-tool.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\星晴\星晴.lnk"
  Delete "$SMPROGRAMS\星晴\卸载星晴.lnk"
  RMDir "$SMPROGRAMS\星晴"
  DeleteRegKey HKCU "Software\Xingqing"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Xingqing"
  ; User projects and application data are intentionally retained.
SectionEnd
