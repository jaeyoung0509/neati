; A product-name change changes NSIS uninstall keys. Never silently install a
; second copy or execute an untrusted legacy UninstallString. The user owns
; removal of the legacy application; its settings and credentials stay put.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Zenith" "Publisher"
  ReadRegStr $R1 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Zenith" "Publisher"
  ${If} $R0 == "jaeyoung0509"
  ${OrIf} $R1 == "jaeyoung0509"
    IfSilent +2
    MessageBox MB_OK|MB_ICONEXCLAMATION "Uninstall the previous Zenith app from Windows Settings before installing Neati. Keep application settings and credentials. This installer will not remove the old application or run its uninstall command for you."
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend
