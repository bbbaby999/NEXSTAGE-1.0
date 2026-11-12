; Hooks do instalador NSIS do NEXSTAGE (bundle.windows.nsis.installerHooks em tauri.conf.json).
; Depois de instalar, grava o idioma escolhido no seletor do instalador ($LANGUAGE = código LCID do
; Windows) para o app abrir nesse idioma na primeira execução. O app só usa esse valor enquanto o
; usuário não tiver escolhido um idioma em Configurações (atualizações não sobrescrevem a escolha).
!macro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "$INSTDIR\installer-lang.txt" w
  FileWrite $0 "$LANGUAGE"
  FileClose $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\installer-lang.txt"
!macroend
