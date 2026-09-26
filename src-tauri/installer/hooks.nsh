; Tauri NSIS installer hooks. The install flow itself is unchanged.

!macro NSIS_HOOK_PREINSTALL
  ; Browse already appends the product folder; do the same for a typed path, so the
  ; app never spreads its files into a shared folder such as D:\Apps. Existing
  ; installs and updates keep their current directory.
  ${If} $UpdateMode <> 1
  ${AndIfNot} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    Push $R0
    StrCpy $R0 $INSTDIR 1 -1
    ${If} $R0 == "\"
      StrCpy $INSTDIR $INSTDIR -1
    ${EndIf}
    ${GetFileName} $INSTDIR $R0
    ${If} $R0 != "${PRODUCTNAME}"
      StrCpy $INSTDIR "$INSTDIR\${PRODUCTNAME}"
      CreateDirectory $INSTDIR
      SetOutPath $INSTDIR
    ${EndIf}
    Pop $R0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The installer picks Chinese or English from Windows. Saving that choice lets the
  ; uninstaller use it instead of asking with its own language dialog.
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $LANGUAGE
!macroend
