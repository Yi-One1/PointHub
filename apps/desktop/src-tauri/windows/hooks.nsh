; PointHub uninstall hook (keep this file pure ASCII).
;
; The uninstaller confirm page is provided by the Tauri NSIS template and has a
; checkbox whose text we override in windows\SimpChinese.nsh / English.nsh to
; "keep account data". Checked = keep data, unchecked = delete data (default).

!macro NSIS_HOOK_PREUNINSTALL
  ; Kill a running instance first: its files keep the data directory locked.
  nsExec::Exec 'taskkill /F /IM pointhub.exe'
  Pop $0

  ; $DeleteAppDataCheckboxState is set when the confirm page is left.
  ; Empty means the page was skipped (silent uninstall) -> default: delete.
  ${If} $DeleteAppDataCheckboxState == "1"
    nsExec::Exec '"$INSTDIR\pointhub.exe" --purge-app'
  ${Else}
    nsExec::Exec '"$INSTDIR\pointhub.exe" --purge-data'
  ${EndIf}
  Pop $0

  ; Installer-owned registry keys: remove even when the user keeps the data.
  DeleteRegKey HKCU "Software\${MANUFACTURER}\${PRODUCTNAME}"
  DeleteRegKey /ifempty HKCU "Software\${MANUFACTURER}"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
!macroend
