; NSIS hooks run by the Tauri bundler. They stop the optional per-user background core
; (the same CONTROLLER executable started with --core-service) so files are not in use.

!include LogicLib.nsh

!macro CONTROLLER_STOP_CORE
  ; Only a service that published an endpoint under the current state folder was started by a
  ; build that understands --stop-background-core. An older executable would open its GUI and
  ; nsExec would wait for it forever, so it is never invoked otherwise.
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${If} ${FileExists} "$LOCALAPPDATA\CONTROLLER\service\endpoint.json"
    ${OrIf} ${FileExists} "$LOCALAPPDATA\CONTROLLER\service-no-hid\endpoint.json"
      ; Asks the service to exit and waits (bounded) until it has.
      nsExec::Exec '"$INSTDIR\${MAINBINARYNAME}.exe" --stop-background-core'
      Pop $0
      Sleep 500
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro CONTROLLER_STOP_CORE
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CONTROLLER_STOP_CORE
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
