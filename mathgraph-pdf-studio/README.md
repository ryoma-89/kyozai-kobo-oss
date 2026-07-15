# MathGraph PDF Studio

数式・不等式・点・ラベルをグラフ化し、教材用A4 PDF（白黒印刷向け）や
PNG/SVG/TikZ（pgfplots）として書き出すWindows向けデスクトップアプリです。

このアプリの描画コア（`src/lib`, `src/components` の一部）は
[kyozai-kobo](../kyozai-kobo) から直接ソースを共有・importされています。
そのため本リポジトリでは `kyozai-kobo/` と `mathgraph-pdf-studio/` を
兄弟ディレクトリとして同じ場所に置く必要があります。

## 技術構成

| 項目 | 内容 |
| --- | --- |
| フレームワーク | Tauri v2 + React + TypeScript + Vite |
| UI | Tailwind CSS / KaTeX・MathJax（数式表示） |
| PDF/SVG出力 | jsPDF / svg2pdf.js |
| 日本語フォント | IPAexゴシック（同梱、`src/assets/fonts`） |

数式の入力ルールは [docs/入力ルール.md](docs/入力ルール.md) を参照してください。

## セットアップ（開発環境）

### 必要なもの

1. Node.js（LTS推奨）
2. Rust（rustup、MSVCツールチェーン）
3. Visual Studio Build Tools（「C++によるデスクトップ開発」ワークロード + Windows SDK）
4. WebView2 ランタイム（Windows 11には標準搭載）

### 起動（開発モード）

```powershell
cd mathgraph-pdf-studio
npm install
npm run tauri dev
```

### リリースビルド

```powershell
npm run tauri build
```

生成物: `src-tauri\target\release\bundle\nsis\MathGraph PDF Studio_0.1.0_x64-setup.exe`

## ライセンス

[../LICENSE](../LICENSE)（PolyForm Noncommercial License 1.0.0）を参照してください。
個人利用・非商用利用は自由ですが、商用利用はできません。
