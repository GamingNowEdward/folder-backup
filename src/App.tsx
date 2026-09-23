import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";

import TitleBar from "./components/TitleBar";
import Sidebar from "./components/Sidebar";
import StatusBar from "./components/StatusBar";
import Toast, { type ToastItem } from "./components/Toast";
import ProgressOverlay from "./components/ProgressOverlay";

import ProjectsPage from "./pages/ProjectsPage";
import ProjectDetailPage from "./pages/ProjectDetailPage";
import SettingsPage from "./pages/SettingsPage";

import ModApplyPreview from "./dialogs/ModApplyPreview";
import RollbackPreviewDialog from "./dialogs/RollbackPreview";
import Confirm from "./dialogs/Confirm";
import ProjectDialog from "./dialogs/ProjectDialog";

import {
  applyMod,
  createProject,
  getSettings,
  listLayers,
  listProjects,
  previewApply,
  previewRollback,
  removeLayer,
  removeProject,
  rollbackTop,
  toAppError,
} from "./lib/ipc";
import type {
  ApplyPreview,
  LayerMeta,
  Page,
  ProgressEvent,
  ProjectMeta,
  RollbackPreview,
} from "./lib/types";

type DialogState =
  | { type: "project" }
  | {
      type: "confirm-delete";
      project: ProjectMeta;
    }
  | { type: "confirm-delete-layer"; layer: LayerMeta }
  | { type: "apply-preview"; preview: ApplyPreview; modSrc: string }
  | { type: "rollback-preview"; preview: RollbackPreview }
  | null;

export default function App() {
  const [page, setPage] = useState<Page>({ name: "projects" });
  const [projects, setProjects] = useState<ProjectMeta[]>([]);
  const [layers, setLayers] = useState<LayerMeta[]>([]);
  const [depthById, setDepthById] = useState<Record<string, number>>({});
  const [dialog, setDialog] = useState<DialogState>(null);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [busy, setBusy] = useState(false);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [status, setStatus] = useState("就绪");
  const toastId = useRef(0);

  const showToast = useCallback((message: string, kind: "success" | "error" = "success", hint?: string) => {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, message, kind, hint }]);
    window.setTimeout(() => {
      setToasts((t) => t.filter((x) => x.id !== id));
    }, 3600);
  }, []);

  const fail = useCallback(
    (err: unknown, fallback: string) => {
      const e = toAppError(err);
      showToast(e.message || fallback, "error", e.hint || undefined);
      setStatus(`错误：${e.message}`);
    },
    [showToast],
  );

  const refreshProjects = useCallback(async () => {
    try {
      const list = await listProjects();
      setProjects(list);
      const depths: Record<string, number> = {};
      await Promise.all(
        list.map(async (p) => {
          try {
            const ls = await listLayers(p.id);
            depths[p.id] = ls.filter((l) => l.status !== "rolled_back").length;
          } catch {
            depths[p.id] = 0;
          }
        }),
      );
      setDepthById(depths);
      return list;
    } catch (err) {
      fail(err, "加载项目失败");
      return [];
    }
  }, [fail]);

  const refreshLayers = useCallback(
    async (projectId: string): Promise<LayerMeta[]> => {
      try {
        const ls = await listLayers(projectId);
        setLayers(ls);
        setDepthById((d) => ({
          ...d,
          [projectId]: ls.filter((l) => l.status !== "rolled_back").length,
        }));
        return ls;
      } catch (err) {
        fail(err, "加载层栈失败");
        return [];
      }
    },
    [fail],
  );

  useEffect(() => {
    void refreshProjects();
    void getSettings()
      .then((s) => setStatus(`备份根：${s.backup_root}`))
      .catch(() => undefined);
    const un = listen<ProgressEvent>("progress", (e) => {
      setProgress(e.payload);
      if (e.payload.total > 0) {
        setStatus(`${e.payload.stage} ${e.payload.done}/${e.payload.total}`);
      } else {
        setStatus(e.payload.stage);
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [refreshProjects]);

  const currentProject =
    page.name === "detail" ? projects.find((p) => p.id === page.projectId) : undefined;

  useEffect(() => {
    if (page.name === "detail") {
      void refreshLayers(page.projectId);
    }
  }, [page, refreshLayers]);

  // ── 动作 ──────────────────────────────

  async function handleCreateProject(name: string, baseDir: string): Promise<void> {
    setBusy(true);
    try {
      await createProject(name, baseDir);
      setDialog(null);
      await refreshProjects();
      showToast(`已创建项目「${name}」`);
      setStatus(`已创建项目 ${name}`);
    } catch (err) {
      fail(err, "创建项目失败");
    } finally {
      setBusy(false);
    }
  }

  async function handleDeleteProject(purge: boolean): Promise<void> {
    if (dialog?.type !== "confirm-delete") return;
    const p = dialog.project;
    setBusy(true);
    try {
      await removeProject(p.id, purge);
      setDialog(null);
      if (page.name === "detail" && page.projectId === p.id) {
        setPage({ name: "projects" });
        setLayers([]);
      }
      await refreshProjects();
      showToast(`已删除项目「${p.name}」${purge ? "（含层数据）" : "（仅索引）"}`);
    } catch (err) {
      fail(err, "删除项目失败");
    } finally {
      setBusy(false);
    }
  }

  async function handleDeleteLayer(): Promise<void> {
    if (dialog?.type !== "confirm-delete-layer" || !currentProject) return;
    const layer = dialog.layer;
    setBusy(true);
    try {
      await removeLayer(currentProject.id, layer.seq);
      setDialog(null);
      await refreshLayers(currentProject.id);
      showToast(`已删除第 ${layer.seq} 层（${layer.mod_name}），层目录已清理`);
      setStatus(`已删除第 ${layer.seq} 层`);
    } catch (err) {
      fail(err, "删除层失败");
      if (currentProject) await refreshLayers(currentProject.id);
    } finally {
      setBusy(false);
    }
  }

  async function startApplyFlow(): Promise<void> {
    if (!currentProject) return;
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    setBusy(true);
    setProgress({ op: "apply", stage: "扫描", done: 0, total: 0 });
    try {
      const preview = await previewApply(currentProject.id, picked);
      setDialog({ type: "apply-preview", preview, modSrc: picked });
    } catch (err) {
      fail(err, "预览失败");
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function confirmApply(): Promise<void> {
    if (dialog?.type !== "apply-preview" || !currentProject) return;
    const { preview, modSrc } = dialog;
    setBusy(true);
    setProgress({ op: "apply", stage: "扫描", done: 0, total: 0 });
    try {
      const meta = await applyMod(currentProject.id, modSrc);
      setDialog(null);
      await refreshLayers(currentProject.id);
      await refreshProjects();
      showToast(
        `已应用层 ${meta.seq}：覆盖 ${meta.overwritten.length} / 新增 ${meta.added.length}`,
      );
      setStatus(`已应用层 ${meta.seq}`);
      void preview;
    } catch (err) {
      const e = toAppError(err);
      fail(err, "应用失败");
      if (currentProject) await refreshLayers(currentProject.id);
      void e;
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function startRollbackFlow(): Promise<void> {
    if (!currentProject) return;
    setBusy(true);
    setProgress({ op: "rollback", stage: "扫描", done: 0, total: 0 });
    try {
      const preview = await previewRollback(currentProject.id);
      setDialog({ type: "rollback-preview", preview });
    } catch (err) {
      fail(err, "恢复预览失败");
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  async function confirmRollback(): Promise<void> {
    if (!currentProject) return;
    setBusy(true);
    setProgress({ op: "rollback", stage: "恢复文件", done: 0, total: 0 });
    try {
      const meta = await rollbackTop(currentProject.id);
      setDialog(null);
      const refreshed = await refreshLayers(currentProject.id);
      await refreshProjects();
      const active = refreshed.filter((l) => l.status !== "rolled_back");
      const nextTop = active.length > 0 ? active[active.length - 1] : undefined;
      const msg = nextTop
        ? `已恢复第 ${meta.seq} 层（${meta.mod_name}），当前顶层 → 第 ${nextTop.seq} 层`
        : `已恢复第 ${meta.seq} 层，层栈已清空，底包已回到应用前状态`;
      showToast(msg);
      setStatus(`已恢复第 ${meta.seq} 层`);
    } catch (err) {
      fail(err, "恢复失败");
      if (currentProject) await refreshLayers(currentProject.id);
    } finally {
      setBusy(false);
      setProgress(null);
    }
  }

  // ── 渲染 ──────────────────────────────

  let main: React.ReactNode;
  if (page.name === "settings") {
    main = (
      <SettingsPage
        onSaved={(root) => {
          showToast("设置已保存");
          setStatus(`备份根：${root}`);
        }}
      />
    );
  } else if (page.name === "detail" && currentProject) {
    main = (
      <ProjectDetailPage
        project={currentProject}
        layers={layers}
        onBack={() => setPage({ name: "projects" })}
        onApply={() => void startApplyFlow()}
        onRollback={() => void startRollbackFlow()}
        onDelete={() => setDialog({ type: "confirm-delete", project: currentProject })}
        onDeleteLayer={(layer) => setDialog({ type: "confirm-delete-layer", layer })}
        busy={busy}
      />
    );
  } else {
    main = (
      <div className="page" data-testid="projects-page">
        <div className="main-header">
          <div className="main-title">项目</div>
          <div className="header-actions">
            <button className="btn btn-accent" onClick={() => setDialog({ type: "project" })}>
              ＋ 新建项目
            </button>
          </div>
        </div>
        <div className="content-scroll">
          <ProjectsPage
            projects={projects}
            depthById={depthById}
            onOpen={(id) => setPage({ name: "detail", projectId: id })}
            onDelete={(p) => setDialog({ type: "confirm-delete", project: p })}
            onCreate={() => setDialog({ type: "project" })}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="app-root" data-testid="app-root">
      <TitleBar />
      <div className="app-body">
        <Sidebar
          page={page}
          projects={projects}
          depthById={depthById}
          onSelectPage={setPage}
          onSelectProject={(id) => setPage({ name: "detail", projectId: id })}
          onAddProject={() => setDialog({ type: "project" })}
        />
        <div className="main-area">{main}</div>
      </div>
      <StatusBar left={status} right={`项目 ${projects.length}`} />
      <Toast toasts={toasts} />
      <ProgressOverlay progress={progress} />

      {dialog?.type === "project" && (
        <ProjectDialog
          mode="create"
          onConfirm={(name, dir) => void handleCreateProject(name, dir)}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "confirm-delete" && (
        <Confirm
          title="删除项目"
          message={`确认删除项目「${dialog.project.name}」？\n默认只删除索引，层数据保留在磁盘；勾选下方选项可一并清除。`}
          confirmLabel="删除"
          withPurge
          onConfirm={(purge) => void handleDeleteProject(purge)}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "confirm-delete-layer" && (
        <Confirm
          title="删除已恢复的层"
          message={`确认删除第 ${dialog.layer.seq} 层（${dialog.layer.mod_name}）？\n该层备份文件将一并删除，不可恢复。`}
          confirmLabel="删除"
          onConfirm={() => void handleDeleteLayer()}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "apply-preview" && (
        <ModApplyPreview
          preview={dialog.preview}
          busy={busy}
          onConfirm={() => void confirmApply()}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.type === "rollback-preview" && (
        <RollbackPreviewDialog
          preview={dialog.preview}
          busy={busy}
          onConfirm={() => void confirmRollback()}
          onCancel={() => setDialog(null)}
        />
      )}
    </div>
  );
}
