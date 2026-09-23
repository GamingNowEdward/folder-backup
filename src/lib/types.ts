export interface ProjectMeta {
  id: string;
  name: string;
  base_path: string;
  created_at: string;
}

export type LayerStatus =
  | "creating"
  | "backed_up"
  | "applied"
  | "restoring"
  | "rolled_back";

export interface StructEntry {
  rel_path: string;
  size: number;
  mtime_ns: number;
}

export interface LayerStats {
  overwrite_bytes: number;
  add_bytes: number;
}

export interface LayerMeta {
  seq: number;
  id: string;
  created_at: string;
  mod_src: string;
  mod_name: string;
  note?: string | null;
  status: LayerStatus;
  structure_before: StructEntry[];
  overwritten: string[];
  added: string[];
  stats: LayerStats;
}

export interface ApplyPreview {
  overwrite_count: number;
  add_count: number;
  untouched_count: number;
  overwrite_bytes: number;
  add_bytes: number;
  overwrite_paths: string[];
  add_paths: string[];
}

export interface RollbackPreview {
  layer_seq: number;
  layer_id: string;
  restore_count: number;
  delete_count: number;
  /** 将清理的本层新增空目录数（旧层为 0） */
  empty_dirs_count: number;
  restore_paths: string[];
  delete_paths: string[];
  empty_dirs: string[];
}

export interface AppError {
  message: string;
  kind: string;
  hint: string;
}

export interface Settings {
  backup_root: string;
}

export interface ProgressEvent {
  op: string;
  stage: string;
  done: number;
  total: number;
  /** 扫描阶段附加信息，如「底包」「Mod」 */
  detail?: string;
}

export type Page =
  | { name: "projects" }
  | { name: "detail"; projectId: string }
  | { name: "settings" };
