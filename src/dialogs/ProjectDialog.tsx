import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

export default function ProjectDialog({
  mode,
  defaultName,
  defaultPath,
  onConfirm,
  onCancel,
}: {
  mode: "create" | "edit-view";
  defaultName?: string;
  defaultPath?: string;
  onConfirm: (name: string, baseDir: string) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(defaultName ?? "");
  const [baseDir, setBaseDir] = useState(defaultPath ?? "");

  useEffect(() => {
    // 仅初始渲染同步
  }, []);

  async function browse(): Promise<void> {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setBaseDir(picked);
  }

  const valid = name.trim().length > 0 && baseDir.trim().length > 0;

  return (
    <div className="dialog-overlay" data-testid="project-dialog">
      <div className="dialog-panel">
        <div className="dialog-title">
          {mode === "create" ? "新建项目" : "项目"}
        </div>
        <label className="dialog-label">项目名</label>
        <input
          className="dialog-input"
          value={name}
          placeholder="例如 MyGame"
          onChange={(e) => setName(e.target.value)}
          disabled={mode === "edit-view"}
        />
        <label className="dialog-label" style={{ marginTop: 14 }}>
          底包目录（游戏本体 folder_base）
        </label>
        <div className="field-row">
          <input
            className="dialog-input"
            value={baseDir}
            placeholder="D:\Games\MyGame"
            onChange={(e) => setBaseDir(e.target.value)}
            disabled={mode === "edit-view"}
          />
          <button className="btn-browse" onClick={() => void browse()} disabled={mode === "edit-view"}>
            浏览…
          </button>
        </div>
        <div className="dialog-buttons">
          <button className="btn-cancel" onClick={onCancel}>
            关闭
          </button>
          {mode === "create" && (
            <button
              className="btn-ok"
              disabled={!valid}
              data-testid="project-confirm"
              onClick={() => onConfirm(name.trim(), baseDir.trim())}
            >
              创建
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
