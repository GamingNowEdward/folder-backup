import type { LayerMeta, LayerStats, ProjectMeta } from "../lib/types";
import { formatBytes } from "../lib/ipc";
import { formatTime, statusLabel } from "../lib/format";

/** 层字节行智能显示：覆盖为 0 只显示新增，新增为 0 只显示覆盖。 */
export function formatLayerBytes(stats: LayerStats): string {
  const hasOver = stats.overwrite_bytes > 0;
  const hasAdd = stats.add_bytes > 0;
  if (hasOver && hasAdd) {
    return `${formatBytes(stats.overwrite_bytes)} + ${formatBytes(stats.add_bytes)}`;
  }
  if (hasOver) return `覆盖 ${formatBytes(stats.overwrite_bytes)}`;
  if (hasAdd) return `新增 ${formatBytes(stats.add_bytes)}`;
  return "0 B";
}

export default function ProjectDetailPage({
  project,
  layers,
  onBack,
  onApply,
  onRollback,
  onDelete,
  onDeleteLayer,
  busy,
}: {
  project: ProjectMeta;
  layers: LayerMeta[];
  onBack: () => void;
  onApply: () => void;
  onRollback: () => void;
  onDelete: () => void;
  onDeleteLayer: (layer: LayerMeta) => void;
  busy: boolean;
}) {
  const active = layers.filter((l) => l.status !== "rolled_back");
  const depth = active.length;
  // 新 → 旧
  const ordered = [...layers].sort((a, b) => b.seq - a.seq);
  const topRollable = active.some((l) => l.status === "applied" || l.status === "restoring");

  return (
    <div className="page" data-testid="detail-page">
      <div className="main-header">
        <div style={{ minWidth: 0 }}>
          <button className="back-link" onClick={onBack}>
            ← 项目
          </button>
          <div className="main-title" style={{ display: "flex", alignItems: "center", gap: 10 }}>
            {project.name}
            <span className="depth-badge" data-testid="depth-badge">
              栈深 {depth}
            </span>
          </div>
          <div className="header-sub" title={project.base_path}>
            {project.base_path}
          </div>
        </div>
        <div className="header-actions">
          <button
            className="btn btn-accent"
            onClick={onApply}
            disabled={busy}
            data-testid="apply-mod-btn"
          >
            应用 Mod…
          </button>
          <button
            className="btn"
            onClick={onRollback}
            disabled={busy || !topRollable}
            data-testid="rollback-btn"
          >
            恢复上一层
          </button>
          <button
            className="btn btn-danger"
            onClick={onDelete}
            disabled={busy}
            title="删除项目"
          >
            删除项目
          </button>
        </div>
      </div>

      <div className="content-scroll">
        {ordered.length === 0 ? (
          <div className="empty-hint">
            <div className="empty-icon">🗂</div>
            <div className="empty-text">层栈为空 — 点击「应用 Mod…」创建第一层</div>
          </div>
        ) : (
          <div className="layer-list">
            {(() => {
              // active 保持 seq 升序 → 最后一个为逻辑栈顶
              const topId = active.length > 0 ? active[active.length - 1].id : undefined;
              return ordered.map((l) => (
                <div key={l.id} className="layer-row" data-testid={`layer-${l.seq}`}>
                  <div className="layer-seq">#{String(l.seq).padStart(4, "0")}</div>
                  <div className="layer-main">
                    <div className="layer-mod">
                      {l.mod_name}
                      {l.id === topId && (
                        <span className="top-badge" data-testid="top-layer-badge" style={{ marginLeft: 10 }}>
                          当前顶层
                        </span>
                      )}
                    </div>
                    <div className="layer-sub">
                      {formatTime(l.created_at)} · {l.id}
                      {l.note ? ` · ${l.note}` : ""}
                    </div>
                  </div>
                  <div className="layer-stats">
                    <div>
                      覆盖 {l.overwritten.length} / 新增 {l.added.length}
                    </div>
                    <div className="layer-bytes">{formatLayerBytes(l.stats)}</div>
                  </div>
                  <span className={`status-pill ${l.status}`}>{statusLabel(l.status)}</span>
                  {l.status === "rolled_back" && (
                    <button
                      className="action-btn action-danger"
                      title="删除该已恢复层"
                      data-testid={`delete-layer-${l.seq}`}
                      disabled={busy}
                      onClick={() => onDeleteLayer(l)}
                    >
                      ✕
                    </button>
                  )}
                </div>
              ));
            })()}
          </div>
        )}
      </div>
    </div>
  );
}
