!include nsDialogs.nsh
!include LogicLib.nsh
Var HiRouteLoginChoice
Var HiRouteTrayChoice
Var HiRouteLoginCheckbox
Var HiRouteTrayCheckbox
Var HiRouteArguments

Function HiRouteOptions
  IfSilent 0 +2
    Abort
  nsDialogs::Create 1018
  Pop $0
  ${NSD_CreateLabel} 0 0 100% 32u "HiRoute installs for this account in LocalAppData\Programs\HiRoute. Background service data is kept separately."
  Pop $0
  ${NSD_CreateCheckbox} 0 45u 100% 14u "Start HiRoute and its background service at login"
  Pop $HiRouteLoginCheckbox
  ${If} $HiRouteLoginChoice != 0
    ${NSD_Check} $HiRouteLoginCheckbox
  ${EndIf}
  ${NSD_CreateCheckbox} 0 70u 100% 14u "Show tray icon and keep the window available in the tray"
  Pop $HiRouteTrayCheckbox
  ${If} $HiRouteTrayChoice != 0
    ${NSD_Check} $HiRouteTrayCheckbox
  ${EndIf}
  nsDialogs::Show
FunctionEnd
Function HiRouteOptionsLeave
  ${NSD_GetState} $HiRouteLoginCheckbox $HiRouteLoginChoice
  ${NSD_GetState} $HiRouteTrayCheckbox $HiRouteTrayChoice
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  StrCmp $INSTDIR "$LOCALAPPDATA\Programs\HiRoute" +3
    MessageBox MB_ICONSTOP "HiRoute install directory is invalid. No product files were written." /SD IDOK
    Abort
  nsExec::ExecToStack '"$PLUGINSDIR\hiroute-preflight.exe" --installer-prepare "$PLUGINSDIR\hiroute-package.json"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "HiRoute could not prepare a verified backup. Existing data is preserved." /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  StrCpy $HiRouteArguments ""
  ${If} $HiRouteLoginChoice = 0
    StrCpy $HiRouteArguments "$HiRouteArguments --no-login-start"
  ${EndIf}
  ${If} $HiRouteTrayChoice = 0
    StrCpy $HiRouteArguments "$HiRouteArguments --no-tray"
  ${EndIf}
  nsExec::ExecToStack '"$INSTDIR\hiroute-shell.exe" --installer-install $HiRouteArguments'
  Pop $0
  Pop $1
  ${If} $0 = 20
    IfSilent hiroute_install_failed
    MessageBox MB_YESNO|MB_ICONEXCLAMATION "The older background service cannot drain work. Stopping it may interrupt active requests and Worker tasks. Stop it and continue?" IDNO hiroute_install_failed
    nsExec::ExecToStack '"$INSTDIR\hiroute-shell.exe" --installer-install --allow-legacy-stop $HiRouteArguments'
    Pop $0
    Pop $1
  ${EndIf}
  ${If} $0 != 0
    hiroute_install_failed:
    MessageBox MB_ICONSTOP "HiRoute installation did not complete. Existing data is preserved. $1" /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode != 1
    StrCpy $HiRouteArguments ""
    ${If} $DeleteAppDataCheckboxState = 1
      StrCpy $HiRouteArguments "--remove-data"
    ${EndIf}
    nsExec::ExecToStack '"$INSTDIR\hiroute-shell.exe" --installer-uninstall $HiRouteArguments'
    Pop $0
    Pop $1
    ${If} $0 = 20
      IfSilent hiroute_uninstall_failed
      MessageBox MB_YESNO|MB_ICONEXCLAMATION "The older background service cannot drain work. Stopping it may interrupt active requests and Worker tasks. Stop it and continue uninstalling?" IDNO hiroute_uninstall_failed
      nsExec::ExecToStack '"$INSTDIR\hiroute-shell.exe" --installer-uninstall --allow-legacy-stop $HiRouteArguments'
      Pop $0
      Pop $1
    ${EndIf}
    ${If} $0 != 0
      hiroute_uninstall_failed:
      MessageBox MB_ICONSTOP "HiRoute was not removed. Resolve any Agent restore decisions or active work, then retry. $1" /SD IDOK
      Abort
    ${EndIf}
  ${EndIf}
!macroend
