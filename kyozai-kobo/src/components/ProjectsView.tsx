import { useEffect, useRef, useState } from "react";
import { createProject, deleteProject, duplicateProject, listProjects } from "../api";
import { useApp } from "../store";
import type { ProjectSummary } from "../types";
import { ProjectEditor } from "./ProjectEditor";

/** 教材プロジェクト画面（一覧 or 編集） */
export function ProjectsView() {
  const { selectedProjectId, selectProject, showToast, confirm, setContextName, bumps } = useApp();
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [newName, setNewName] = useState("");
  const seenProjectsBumpRef = useRef(bumps.projects);

  const load = async () => {
    try {
      setProjects(await listProjects());
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  useEffect(() => {
    if (selectedProjectId == null) {
      load();
      setContextName("教材プロジェクト");
    }
    return () => setContextName("");
  }, [selectedProjectId]);

  useEffect(() => {
    if (seenProjectsBumpRef.current === bumps.projects) return;
    seenProjectsBumpRef.current = bumps.projects;
    if (selectedProjectId == null) void load();
  }, [bumps.projects, selectedProjectId]);

  // Ctrl+N で新規プロジェクト（一覧表示時）
  useEffect(() => {
    if (selectedProjectId != null) return;
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        onCreate();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [selectedProjectId, newName]);

  const onCreate = async () => {
    try {
      const id = await createProject(newName);
      setNewName("");
      selectProject(id);
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDuplicate = async (id: number) => {
    try {
      await duplicateProject(id);
      await load();
      showToast("複製しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  const onDelete = async (p: ProjectSummary) => {
    if (!(await confirm(`教材「${p.name}」を削除しますか？`))) return;
    try {
      await deleteProject(p.id);
      await load();
      showToast("削除しました");
    } catch (e) {
      showToast(String(e), "error");
    }
  };

  if (selectedProjectId != null) {
    return <ProjectEditor projectId={selectedProjectId} onBack={() => selectProject(null)} />;
  }

  return (
    <div className="mx-auto h-full max-w-3xl overflow-y-auto px-6 py-5">
      <h1 className="mb-4 text-base font-bold">
        <span className="brand-mark">▤</span> 教材プロジェクト
      </h1>
      <div className="mb-5 flex gap-2">
        <input
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onCreate();
          }}
          className="input flex-1"
          placeholder="新しい教材名（例: 高1数学_二次関数_夏期講習第1回）"
        />
        <button onClick={onCreate} className="btn btn-solid">
          ＋ 作成 (Ctrl+N)
        </button>
      </div>
      {projects.length === 0 ? (
        <p className="text-sm" style={{ color: "var(--muted)" }}>
          まだ教材がありません。
        </p>
      ) : (
        <ul className="space-y-2">
          {projects.map((p) => (
            <li key={p.id} className="card card-glow flex items-center gap-3 px-4 py-3">
              <button onClick={() => selectProject(p.id)} className="min-w-0 flex-1 text-left">
                <div className="truncate font-semibold">{p.name}</div>
                <div className="text-xs" style={{ color: "var(--muted)" }}>
                  {p.item_count}問 ・ 更新 {p.updated_at}
                  {p.description && <span className="ml-2">{p.description}</span>}
                </div>
              </button>
              <button onClick={() => onDuplicate(p.id)} className="btn btn-ghost btn-sm">
                複製
              </button>
              <button onClick={() => onDelete(p)} className="btn btn-danger btn-sm">
                削除
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
