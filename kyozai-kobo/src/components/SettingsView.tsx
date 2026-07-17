import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  createSampleData,
  detectGraphAppPath,
  detectTex,
  getSettings,
  listTemplates,
  setSettings,
  testGraphIntegrationSettings,
} from "../api";
import { useApp } from "../store";
import { isTauri } from "../transport";
import type { TemplateSummary } from "../types";
import { CodexSettings } from "./CodexSettings";
import { ServerSettings } from "./ServerSettings";
import { Icon } from "./Icon";

/** 設定画面 */
export function SettingsView() {
  const { showToast, refreshTree, confirm, setContextName, bumps } = useApp();
  const [values, setValues] = useState<Record<string, string>>({});
  const [loaded, setLoaded] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [templates, setTemplates] = useState<TemplateSummary[]>([]);
  const [localDirty, setLocalDirty] = useState(false);
  const seenSettingsBumpRef = useRef(bumps.settings);
  const pendingSettingsRefreshRef = useRef(false);
  const settingsLoadRequestRef = useRef(0);
  const localDirtyRef = useRef(localDirty);
  localDirtyRef.current = localDirty;

  const loadValues = async (preserveDirty = false) => {
    const requestId = ++settingsLoadRequestRef.current;
    try {
      const next = await getSettings();
      if (requestId !== settingsLoadRequestRef.current) return;
      if (preserveDirty && localDirtyRef.current) {
        pendingSettingsRefreshRef.current = true;
        return;
      }
      setValues(next);
      setLoaded(true);
    } catch (e) {
      if (requestId === settingsLoadRequestRef.current) showToast(String(e), "error");
    }
  };

  useEffect(() => {
    setContextName("設定");
    void loadValues();
    listTemplates().then(setTemplates).catch(() => {});
    return () => setContextName("");
  }, []);

  useEffect(() => {
    if (seenSettingsBumpRef.current === bumps.settings) return;
    seenSettingsBumpRef.current = bumps.settings;
    if (localDirty) {
      pendingSettingsRefreshRef.current = true;
      return;
    }
    void loadValues(true);
  }, [bumps.settings]);

  useEffect(() => {
    if (localDirty || !pendingSettingsRefreshRef.current) return;
    pendingSettingsRefreshRef.current = false;
    void loadValues(true);
  }, [localDirty]);

  const set = (key: string, value: string) => {
    setValues((v) => ({ ...v, [key]: value }));
    setLocalDirty(true);
  };

  const save = async () => {
    try {
      await setSettings(values);
      setLocalDirty(false);
      showToast("設定を保存しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDetect = async () => {
    setDetecting(true);
    try {
      const d = await detectTex();
      if (d.uplatex_path || d.dvipdfmx_path) {
        setValues((v) => ({
          ...v,
          uplatex_path: d.uplatex_path ?? v.uplatex_path ?? "",
          dvipdfmx_path: d.dvipdfmx_path ?? v.dvipdfmx_path ?? "",
        }));
        setLocalDirty(true);
        showToast("TeXコマンドを検出しました。「保存」を押して確定してください。");
      } else {
        showToast(
          "TeX環境が見つかりませんでした。TeX Live または MiKTeX をインストールするか、下の欄でパスを直接指定してください。",
          "error",
        );
      }
    } catch (e) {
      showToast(String(e), "error");
    } finally {
      setDetecting(false);
    }
  };

  const onDetectGraphApp = async () => {
    try {
      const path = await detectGraphAppPath();
      if (path) {
        set("graph_app_path", path);
        showToast("グラフ作成アプリを検出しました。保存を押して確定してください。");
      } else {
        showToast("グラフ作成アプリを自動検出できませんでした。実行ファイルを指定してください。", "error");
      }
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onTestGraphApp = async () => {
    try {
      await setSettings(values);
      setLocalDirty(false);
      const result = await testGraphIntegrationSettings();
      showToast(
        result.path ? `${result.message}\n${result.path}` : result.message,
        result.ok ? "info" : "error",
      );
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const browse = async (key: string, isDir: boolean) => {
    const picked = await openDialog(
      isDir ? { directory: true } : { filters: [{ name: "実行ファイル", extensions: ["exe"] }] },
    );
    if (picked) set(key, picked as string);
  };

  const onSample = async () => {
    if (!(await confirm("サンプルデータ（数学の問題6問と階層）を追加しますか？"))) return;
    try {
      await createSampleData();
      await refreshTree();
      showToast("サンプルデータを追加しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  if (!loaded)
    return (
      <p className="p-4 text-sm" style={{ color: "var(--muted)" }}>
        読み込み中...
      </p>
    );

  const pathRow = (label: string, key: string, isDir: boolean, placeholder: string) => (
    <div>
      <label className="section-label mb-0.5 block">{label}</label>
      <div className="flex gap-1.5">
        <input
          value={values[key] ?? ""}
          onChange={(e) => set(key, e.target.value)}
          className="input flex-1 font-mono text-xs"
          placeholder={placeholder}
        />
        {isTauri && (
          <button onClick={() => browse(key, isDir)} className="btn btn-ghost btn-sm">
            参照...
          </button>
        )}
      </div>
    </div>
  );

  const sectionTitle = (title: string) => (
    <h2
      className="border-b pb-1 text-sm font-bold"
      style={{ borderColor: "var(--border)", color: "var(--accent)" }}
    >
      {title}
    </h2>
  );

  return (
    <div className="mx-auto h-full max-w-2xl overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-base font-bold">
        <span className="brand-mark"><Icon name="settings" size={16} /></span> 設定
      </h1>

      {isTauri && (
        <section className="mb-6 space-y-3">
          {sectionTitle("教材サーバー（iPad・ブラウザからのアクセス）")}
          <ServerSettings />
        </section>
      )}

      <section className="mb-6 space-y-3">
        {sectionTitle("Codex / ChatGPT接続（AI変換）")}
        <CodexSettings />
      </section>

      <section className="mb-6 space-y-3">
        {sectionTitle("AI解答・解説の参考スタイル")}
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          提供された駿台の「研究問題・問題と解答」は完成解答の簡潔さへ、
          「板書・授業ノート」は着眼点や定石を説明する詳しさへ反映します。
          原文を転載せず、解法の選び方と記述様式を一般化して使用します。
        </p>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={(values["solution_reference_style_enabled"] ?? "1") !== "0"}
            onChange={(e) => set("solution_reference_style_enabled", e.target.checked ? "1" : "0")}
          />
          参考資料に寄せた解答・解説を生成する
        </label>
        <div>
          <label className="section-label mb-0.5 block">追加の書き方指定（任意）</label>
          <textarea
            value={values["solution_reference_custom"] ?? ""}
            onChange={(e) => set("solution_reference_custom", e.target.value.slice(0, 6000))}
            className="input-area h-28 w-full resize-y text-xs"
            placeholder="例：解答は簡潔に、解説では置換を選ぶ理由を特に詳しく書く"
          />
          <p className="mt-1 text-[11px]" style={{ color: "var(--muted)" }}>
            数学的正確さ、高校範囲、生成時に選んだ段組の本文幅、図の自然な配置、「\cdots ①」形式が常に優先されます。
          </p>
        </div>
      </section>

      <section className="mb-6 space-y-3">
        {sectionTitle("TeX環境")}
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          PDF生成には TeX Live または MiKTeX が必要です（コンパイル方式: uplatex + dvipdfmx）。
          未設定でも問題管理・.tex出力は利用できます。
        </p>
        <button onClick={onDetect} disabled={detecting} className="btn btn-outline">
          {detecting ? "検出中..." : "TeXコマンドを自動検出"}
        </button>
        {pathRow("uplatex のパス", "uplatex_path", false, "例: C:\\texlive\\2025\\bin\\windows\\uplatex.exe")}
        {pathRow("dvipdfmx のパス", "dvipdfmx_path", false, "例: C:\\texlive\\2025\\bin\\windows\\dvipdfmx.exe")}
        {pathRow("TeX の bin フォルダ（上2つが空の場合に使用）", "tex_bin_dir", true, "例: C:\\texlive\\2025\\bin\\windows")}
      </section>

      {isTauri && (
        <section className="mb-6 space-y-3">
          {sectionTitle("グラフ作成アプリ連携")}
          <div className="flex flex-wrap gap-2">
            <button onClick={onDetectGraphApp} className="btn btn-outline">
              自動検出
            </button>
            <button onClick={onTestGraphApp} className="btn btn-ghost">
              接続テスト
            </button>
          </div>
          {pathRow(
            "グラフ作成アプリの実行ファイル",
            "graph_app_path",
            false,
            "例: C:\\Program Files\\MathGraph PDF Studio\\mathgraph-pdf-studio.exe",
          )}
          {pathRow(
            "連携用データ保存先",
            "graph_integration_dir",
            true,
            "未設定の場合はアプリデータ内 integrations",
          )}
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="section-label mb-0.5 block">優先出力形式</label>
              <select
                value={values["graph_preferred_output"] ?? "pdf"}
                onChange={(e) => set("graph_preferred_output", e.target.value)}
                className="select w-full"
              >
                <option value="pdf">PDF優先</option>
                <option value="png">PNG優先</option>
                <option value="tex">LaTeXソース優先</option>
              </select>
            </div>
            <div>
              <label className="section-label mb-0.5 block">標準挿入幅</label>
              <input
                value={values["graph_insert_width"] ?? "0.72\\linewidth"}
                onChange={(e) => set("graph_insert_width", e.target.value)}
                className="input w-full font-mono text-xs"
                placeholder="0.72\\linewidth"
              />
            </div>
          </div>
          <div>
            <label className="section-label mb-0.5 block">連携ログ</label>
            <textarea
              readOnly
              value={[
                `graph_app_path=${values["graph_app_path"] ?? ""}`,
                `graph_integration_dir=${values["graph_integration_dir"] ?? "(default)"}`,
                `graph_preferred_output=${values["graph_preferred_output"] ?? "pdf"}`,
                `graph_insert_width=${values["graph_insert_width"] ?? "0.72\\linewidth"}`,
              ].join("\n")}
              className="input-area h-20 w-full resize-none font-mono text-xs"
            />
          </div>
        </section>
      )}

      <section className="mb-6 space-y-3">
        {sectionTitle("問題プレビュー")}
        <p className="text-xs" style={{ color: "var(--muted)" }}>
          問題編集画面の「コンパイル」プレビューは、ここで選んだテンプレートのプリアンブル
          （\usepackage や独自コマンド）を使ってコンパイルされます。教材で使うテンプレートと揃えると、
          プレビューでのコンパイルエラーを防げます。
        </p>
        <div>
          <label className="section-label mb-0.5 block">プレビューに使うテンプレート</label>
          <select
            value={values["preview_template_id"] ?? ""}
            onChange={(e) => set("preview_template_id", e.target.value)}
            className="select w-full"
          >
            <option value="">（先頭のテンプレートを使う）</option>
            {templates.map((t) => (
              <option key={t.id} value={String(t.id)}>
                {t.name}
              </option>
            ))}
          </select>
        </div>
      </section>

      <section className="mb-6 space-y-3">
        {sectionTitle("保存先")}
        {pathRow("PDF・.tex の出力先フォルダ", "output_dir", true, "未設定の場合は ドキュメント\\教材工房")}
        <div>
          <label className="section-label mb-0.5 block">データ保存先（データベース・添付ファイル）</label>
          <input value={values["data_dir"] ?? ""} readOnly className="input w-full font-mono text-xs" />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={(values["auto_backup"] ?? "1") !== "0"}
            onChange={(e) => set("auto_backup", e.target.checked ? "1" : "0")}
          />
          起動時に自動バックアップを作成する（データフォルダ内 backups、最大10世代）
        </label>
      </section>

      <section className="mb-6 space-y-3">
        {sectionTitle("データ")}
        <button onClick={onSample} className="btn btn-ghost">
          サンプルデータを追加
        </button>
      </section>

      <div className="flex justify-end">
        <button onClick={save} className="btn btn-solid">
          保存
        </button>
      </div>
    </div>
  );
}
