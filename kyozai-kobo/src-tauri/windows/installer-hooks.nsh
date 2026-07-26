Var KyozaiKoboExistingInstall

!macro NSIS_HOOK_PREINSTALL
  StrCpy $KyozaiKoboExistingInstall "0"
  IfFileExists "$INSTDIR\kyozai-kobo.exe" 0 +2
    StrCpy $KyozaiKoboExistingInstall "1"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  StrCmp $KyozaiKoboExistingInstall "1" setup_guide_done
  MessageBox MB_OK|MB_ICONINFORMATION "教材工房の追加機能には、次の外部環境が必要です。$\r$\n$\r$\n・PDF生成: TeX Live（uplatex / dvipdfmx）$\r$\n・AI変換、解答・解説生成: OpenAI Codex CLIとChatGPTへの接続$\r$\n・Node.js: Codexをnpmで導入する場合のみ$\r$\n$\r$\nこれらは自動インストールされません。教材工房を起動すると、初回セットアップで現在の環境を確認し、公式の導入手順と必要な設定を案内します。"
  setup_guide_done:
!macroend
