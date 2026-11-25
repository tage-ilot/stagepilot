!macro STAGEPILOT_STOP_BACKEND
  DetailPrint "Stopping the StagePilot backend before replacing application files..."
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM "stagepilot-backend.exe"'
  Sleep 1000
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro STAGEPILOT_STOP_BACKEND
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro STAGEPILOT_STOP_BACKEND
!macroend

