import { useState } from "react";

export default function Confirm({
  title,
  message,
  confirmLabel,
  withPurge,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel?: string;
  withPurge?: boolean;
  onConfirm: (purge: boolean) => void;
  onCancel: () => void;
}) {
  const [purge, setPurge] = useState(false);
  return (
    <div className="dialog-overlay" data-testid="confirm-dialog">
      <div className="dialog-panel">
        <div className="dialog-title">{title}</div>
        <div className="dialog-message">{message}</div>
        {withPurge && (
          <label className="check-row">
            <input
              type="checkbox"
              checked={purge}
              onChange={(e) => setPurge(e.target.checked)}
            />
            同时删除该层栈全部备份数据（--purge）
          </label>
        )}
        <div className="dialog-buttons">
          <button className="btn-cancel" onClick={onCancel}>
            取消
          </button>
          <button
            className="btn-ok"
            data-testid="confirm-ok"
            onClick={() => onConfirm(purge)}
          >
            {confirmLabel ?? "确认"}
          </button>
        </div>
      </div>
    </div>
  );
}
