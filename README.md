# kyozai-kobo-oss

塾教材をLaTeXで作成・管理するWindows向けデスクトップアプリ「**教材工房**」と、
その中から呼び出せるグラフ作成アプリ「**MathGraph PDF Studio**」をまとめたリポジトリです。

> **ライセンス: [PolyForm Noncommercial License 1.0.0](LICENSE)**
> 個人利用・研究・学習・非営利団体での利用は自由に行えますが、**商用利用はできません**。
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
