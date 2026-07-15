# サードパーティ ライセンス表記

このリポジトリは以下のオフライン同梱アセットを含みます。それぞれのライセンスは各プロジェクトの元配布元に従います。

## kyozai-kobo

- **MathJax** (`kyozai-kobo/public/mathjax/tex-svg.js`, `tex-svg-full.js`)
  Apache License 2.0. <https://github.com/mathjax/MathJax/blob/master/LICENSE>
- **pdf.js** (`kyozai-kobo/public/pdfjs/`)
  Apache License 2.0. 同梱の `LICENSE` / `LICENSE_FOXIT` / `LICENSE_LIBERATION` /
  `LICENSE_JBIG2` / `LICENSE_OPENJPEG` / `LICENSE_QCMS` 等を参照。

## mathgraph-pdf-studio

- **IPAexゴシック** (`mathgraph-pdf-studio/src/assets/fonts/ipaexg.ttf`)
  IPAフォントライセンスv1.0。同梱の `IPA_Font_License_Agreement_v1.0.txt` および
  `Readme_ipaexg00401.txt` を参照。

npmパッケージ依存（React, Tauri, KaTeX, mathjs, three.js 等）はいずれもMIT /
Apache-2.0など再配布・改変・商用利用を制限しない緩いライセンスです。詳細は各
`package.json` / `Cargo.toml` の依存関係と、それぞれのパッケージのライセンス
表記を参照してください。
