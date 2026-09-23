import type { ProgressEvent } from "../lib/types";

export default function ProgressOverlay({ progress }: { progress: ProgressEvent | null }) {
  if (!progress) return null;
  const indeterminate = progress.total <= 0;
  const pct = indeterminate
    ? 0
    : Math.min(100, Math.round((progress.done / progress.total) * 100));

  const stageLabel =
    progress.stage === "扫描" && progress.detail
      ? `扫描${progress.detail}`
      : progress.stage;

  let countLabel: string;
  if (progress.total > 0) {
    countLabel = `${progress.done} / ${progress.total}`;
  } else if (progress.done > 0) {
    countLabel = `已发现 ${progress.done} 项`;
  } else {
    countLabel = "请稍候…";
  }

  return (
    <div className="dialog-overlay" data-testid="progress-overlay">
      <div className="dialog-panel progress-panel">
        <div className="dialog-title">处理中</div>
        <div className="progress-stage">{stageLabel}</div>
        <div className="progress-track">
          <div
            className={`progress-fill ${indeterminate ? "indeterminate" : ""}`}
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="progress-count">{countLabel}</div>
      </div>
    </div>
  );
}
