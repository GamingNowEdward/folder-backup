import type { RollbackPreview } from "../lib/types";
import { PathList } from "./ModApplyPreview";

export default function RollbackPreviewDialog({
  preview,
  onConfirm,
  onCancel,
  busy,
}: {
  preview: RollbackPreview;
  onConfirm: () => void;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <div className="dialog-overlay" data-testid="rollback-preview">
      <div className="dialog-panel wide">
        <div className="dialog-title">预览：恢复第 {preview.layer_seq} 层</div>
        <div className="preview-stats">
          将恢复 {preview.restore_count} 个文件／将删除 {preview.delete_count} 个本层新增文件
        </div>
        <div className="remind">请先关闭游戏，再继续恢复。</div>
        <PathList
          label="将恢复"
          count={preview.restore_count}
          paths={preview.restore_paths}
        />
        <PathList label="将删除" count={preview.delete_count} paths={preview.delete_paths} />
        <PathList
          label="将清理的空目录"
          count={preview.empty_dirs_count}
          paths={preview.empty_dirs}
        />
        <div className="dialog-buttons">
          <button className="btn-cancel" onClick={onCancel} disabled={busy}>
            取消
          </button>
          <button className="btn-ok" onClick={onConfirm} disabled={busy} data-testid="rollback-confirm">
            {busy ? "恢复中…" : "确认恢复"}
          </button>
        </div>
      </div>
    </div>
  );
}
