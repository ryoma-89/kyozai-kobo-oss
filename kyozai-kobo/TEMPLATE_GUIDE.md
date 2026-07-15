# 教材工房 テンプレートガイド

テンプレートの詳しい説明は [USER_GUIDE.md の「LaTeX テンプレート詳説」](USER_GUIDE.md#8-latex-テンプレート詳説) にまとめています。

最低限の考え方は次の通りです。

- 問題冊子用テンプレートには `{{BODY}}` を置きます。
- 解答冊子用テンプレートには `{{ANSWER_BODY}}` を置きます。
- 解答冊子のタイトルは `{{ANSWER_TITLE}}` を使うと、出力設定の「教材タイトルを表示する」と連動します。
- 目次位置を指定したい場合は `{{TOC}}` を置きます。
- 既存 `.tex` では `% APP_BODY_START` と `% APP_BODY_END` の間に本文を挿入できます。
- パッケージを追加する場合は、テンプレート本文のプリアンブルに `\usepackage{...}` を書きます。テンプレート画面のパッケージメモ欄は記録用です。

## よく使うプレースホルダー

| プレースホルダー | 内容 |
| --- | --- |
| `{{TITLE}}` | 教材タイトル |
| `{{ANSWER_TITLE}}` | 解答冊子用タイトル |
| `{{SUBTITLE}}` | 副題 |
| `{{TARGET}}` | 学年、対象 |
| `{{DATE}}` | 日付 |
| `{{NAME_FIELD}}` | 氏名欄 |
| `{{HEADER_LEFT}}` | ヘッダー左 |
| `{{HEADER_RIGHT}}` | ヘッダー右 |
| `{{BODY}}` | 問題冊子本文 |
| `{{ANSWER_BODY}}` | 解答冊子本文 |
| `{{EXPLANATION_BODY}}` | 解説だけを別位置に出す場合の本文 |
| `{{TOC}}` | 目次 |
| `{{PAGE_BREAK}}` | `\newpage` |

## 最小テンプレート

問題冊子用:

```tex
\documentclass[uplatex,a4paper,11pt]{ujarticle}
\usepackage[dvipdfmx]{graphicx}
\usepackage{amsmath,amssymb,mathtools}

\begin{document}
{\LARGE \bfseries {{TITLE}}\par}

{{BODY}}

\end{document}
```

解答冊子用:

```tex
\documentclass[uplatex,a4paper,11pt]{ujarticle}
\usepackage[dvipdfmx]{graphicx}
\usepackage{amsmath,amssymb,mathtools}

\begin{document}
{\LARGE \bfseries {{ANSWER_TITLE}}\par}

{{ANSWER_BODY}}

\end{document}
```
