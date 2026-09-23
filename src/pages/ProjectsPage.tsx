import type { ProjectMeta } from "../lib/types";

export default function ProjectsPage({
  projects,
  depthById,
  onOpen,
  onDelete,
  onCreate,
}: {
  projects: ProjectMeta[];
  depthById: Record<string, number>;
  onOpen: (id: string) => void;
  onDelete: (p: ProjectMeta) => void;
  onCreate: () => void;
}) {
  if (projects.length === 0) {
    return (
      <div className="empty-hint" data-testid="projects-empty">
        <div className="empty-icon">📂</div>
        <div className="empty-text">暂无项目</div>
        <button className="btn btn-accent" style={{ marginTop: 8 }} onClick={onCreate}>
          ＋ 新建项目
        </button>
      </div>
    );
  }

  return (
    <div className="cards-grid" data-testid="projects-grid">
      {projects.map((p) => (
        <div
          key={p.id}
          className="folder-card"
          data-testid={`project-card-${p.name}`}
          onClick={() => onOpen(p.id)}
        >
          <div className="card-top">
            <div className="card-icon">🎮</div>
            <div className="card-info">
              <div className="card-name">{p.name}</div>
              <div className="card-path" title={p.base_path}>
                {p.base_path}
              </div>
            </div>
          </div>
          <div className="card-meta">
            <span>栈深 {depthById[p.id] ?? 0}</span>
            <span>{new Date(p.created_at).toLocaleDateString()}</span>
          </div>
          <div className="card-actions">
            <button
              className="action-btn"
              title="打开"
              onClick={(e) => {
                e.stopPropagation();
                onOpen(p.id);
              }}
            >
              ▶
            </button>
            <button
              className="action-btn action-danger"
              title="删除"
              onClick={(e) => {
                e.stopPropagation();
                onDelete(p);
              }}
            >
              ✕
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
