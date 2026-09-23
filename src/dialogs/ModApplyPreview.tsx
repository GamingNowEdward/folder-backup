import type { ApplyPreview } from "../lib/types";
import { formatBytes, hiddenCount } from "../lib/ipc";

export function PathList({ label, count, paths }: { label: string; count: number; paths: string[] }) {
  if (count <= 0) return null;
  const hidden = hiddenCount(count, paths.length);
  return (
    <div className="path-block">
      <div className="path-block-label">
        {label}（{count}）
      </div>
      <ul className="path-list">
        {paths.map((p) => (
          <li key={p}>{p}</li>
        ))}
      </ul>
      {hidden > 0 && <div className="path-more">… 另有 {hidden} 条未展示</div>}
    </div>
  );
}

export default function ModApplyPreview({
  preview,
  onConfirm,
  onCancel,
  busy,
}: {
  preview: ApplyPreview;
  onConfirm: () => void;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <div className="dialog-overlay" data-testid="apply-preview">
      <div className="dialog-panel wide">
        <div className="dialog-title">预览：应用 Mod</div>
        <div className="preview-stats">
          覆盖 {preview.overwrite_count} 个（{formatBytes(preview.overwrite_bytes)}）／新增{" "}
          {preview.add_count} 个（{formatBytes(preview.add_bytes)}）／未触及{" "}
          {preview.untouched_count} 个
        </div>
        <div className="remind">请先关闭游戏，再继续应用。</div>
        <PathList label="将被覆盖" count={preview.overwrite_count} paths={preview.overwrite_paths} />
        <PathList label="将新增" count={preview.add_count} paths={preview.add_paths} />
        <div className="dialog-buttons">
          <button className="btn-cancel" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button className="btn-ok" onClick={onConfirm} disabled={busy} data-testid="apply-confirm">
            {busy ? "应用中…" : "确认应用"}
          </button>
        </div>
      </div>
    </div>
  );
}
