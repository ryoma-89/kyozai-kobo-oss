# kyozai-kobo-oss

[![License: PolyForm Internal Use 1.0.0](https://img.shields.io/badge/License-PolyForm%20Internal%20Use%201.0.0%20%2B%20Additional%20Permissions-blue.svg)](LICENSE)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](#)
[![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri%20v2-24C8DB.svg)](https://tauri.app)

塾教材をLaTeXで作成・管理するWindows向けデスクトップアプリ「**教材工房**」と、
その中から呼び出せるグラフ作成アプリ「**MathGraph PDF Studio**」をまとめたリポジトリです。

> **ライセンス: [PolyForm Internal Use License 1.0.0 + 追加許諾](LICENSE)**
>
> - ✅ **使うのは自由**: 個人でも、塾・学校などの業務でも、このソフトで教材を作れます
> - ✅ **作った教材はあなたのもの**: 生成した教材・PDF・`.tex`・図表は、有料授業での配布や販売を含め**商用でも自由に利用できます**
> - ✅ **自分用の改造は自由**
> - ❌ **ソフト自体の再配布・販売は不可**: 本体・改変版・ビルド済みEXEを第三者へ配布できません
>
> 詳細条件は [LICENSE](LICENSE) を必ず確認してください。

## 構成

| ディレクトリ | 内容 |
| --- | --- |
| [kyozai-kobo/](kyozai-kobo) | 本体アプリ。問題バンク・教材編成・LaTeX/PDF生成・iPad等からのWebアクセス |
| [mathgraph-pdf-studio/](mathgraph-pdf-studio) | グラフ作成アプリ。教材工房から呼び出して使うほか単独でも動作 |

`kyozai-kobo` は `mathgraph-pdf-studio` のグラフ描画コア（TypeScriptソース）を
相対パスで直接importして共有しています。そのため**この2つのディレクトリは
兄弟関係のまま同じ場所に置いてください**（片方だけを取り出すとビルドできません）。

セットアップ・使い方はそれぞれの README を参照してください。

- [kyozai-kobo/README.md](kyozai-kobo/README.md)（教材工房 本体、Web版・AI変換の説明含む）
- [kyozai-kobo/USER_GUIDE.md](kyozai-kobo/USER_GUIDE.md)（テンプレートの使い方）
- [mathgraph-pdf-studio/README.md](mathgraph-pdf-studio/README.md)

## 前提となる外部ツール（同梱していません）

- **TeX Live または MiKTeX**: 教材工房のLaTeX→PDF変換（`uplatex` + `dvipdfmx`）に必要。
  未導入でも問題管理・教材編成・`.tex`書き出しは利用できます。
- **Codex CLI（`@openai/codex`）**: 写真・テキストからのAI変換機能を使う場合のみ必要。
  利用にはご自身のChatGPT/OpenAIアカウントが必要です（本リポジトリに認証情報は含まれません）。

## サードパーティ ライセンス

MathJax・pdf.js・IPAexフォント等の同梱アセットについては
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。
