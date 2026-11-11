; Always create a desktop shortcut, including silent installations.
!macro NSIS_HOOK_POSTINSTALL
  CreateShortcut "$DESKTOP\DayNote.lnk" "$INSTDIR\DayNote.exe" "" "$INSTDIR\DayNote.exe" 0
!macroend

; Task databases live outside the install directory and are deliberately retained.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$DESKTOP\DayNote.lnk"
!macroend
