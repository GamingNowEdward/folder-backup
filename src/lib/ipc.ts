import { invoke } from "@tauri-apps/api/core";
import type {
  AppError,
  ApplyPreview,
  LayerMeta,
  ProjectMeta,
  RollbackPreview,
  Settings,
} from "./types";

/** 把任意异常规整为 AppError（kind/hint 中文建议）。 */
export function toAppError(err: unknown): AppError {
  if (err && typeof err === "object" && "message" in err && "kind" in err) {
    const e = err as AppError;
    if (typeof e.kind === "string" && typeof e.hint === "string") {
      return e;
    }
  }
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : JSON.stringify(err);
  return { message, kind: "other", hint: "" };
}

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function setSettings(backupRoot: string): Promise<Settings> {
  return invoke<Settings>("set_settings", { backupRoot });
}

export async function listProjects(): Promise<ProjectMeta[]> {
  return invoke<ProjectMeta[]>("list_projects");
}

export async function createProject(
  name: string,
  baseDir: string,
): Promise<ProjectMeta> {
  return invoke<ProjectMeta>("create_project", { name, baseDir });
}

export async function removeProject(
  projectId: string,
  purge: boolean,
): Promise<void> {
  return invoke<void>("remove_project", { projectId, purge });
}

export async function previewApply(
  projectId: string,
  modSrc: string,
): Promise<ApplyPreview> {
  return invoke<ApplyPreview>("preview_apply", { projectId, modSrc });
}

export async function applyMod(
  projectId: string,
  modSrc: string,
  note?: string,
): Promise<LayerMeta> {
  return invoke<LayerMeta>("apply_mod", { projectId, modSrc, note: note ?? null });
}

export async function listLayers(projectId: string): Promise<LayerMeta[]> {
  return invoke<LayerMeta[]>("list_layers", { projectId });
}

export async function previewRollback(
  projectId: string,
): Promise<RollbackPreview> {
  return invoke<RollbackPreview>("preview_rollback", { projectId });
}

export async function rollbackTop(projectId: string): Promise<LayerMeta> {
  return invoke<LayerMeta>("rollback_top", { projectId });
}

export async function removeLayer(
  projectId: string,
  seq: number,
): Promise<LayerMeta> {
  return invoke<LayerMeta>("remove_layer", { projectId, seq });
}

export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i + 1 < units.length) {
    v /= 1024;
    i += 1;
  }
  if (i === 0) return `${n} B`;
  return `${v.toFixed(1)} ${units[i]}`;
}

/** 计数超列表长度时的「另有 x 条」；否则 0。 */
export function hiddenCount(count: number, shown: number): number {
  return Math.max(0, count - shown);
}
