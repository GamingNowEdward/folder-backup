import type { Page, ProjectMeta } from "../lib/types";

interface Props {
  page: Page;
  projects: ProjectMeta[];
  depthById: Record<string, number>;
  onSelectPage: (page: Page) => void;
  onSelectProject: (id: string) => void;
  onAddProject: () => void;
}

export default function Sidebar({
  page,
  projects,
  depthById,
  onSelectPage,
  onSelectProject,
  onAddProject,
}: Props) {
  const onProjectsNav = page.name !== "settings";
  const inDetail = page.name === "detail";

  return (
    <div className="sidebar" data-testid="sidebar">
      <div className="sidebar-label">导航</div>
      <div
        className={`sidebar-item ${onProjectsNav ? "active" : ""}`}
        onClick={() => onSelectPage({ name: "projects" })}
      >
        <span className="action-icon">▦</span>项目
      </div>
      <div
        className={`sidebar-item ${page.name === "settings" ? "active" : ""}`}
        onClick={() => onSelectPage({ name: "settings" })}
      >
        <span className="action-icon">⚙</span>设置
      </div>

      <div className="sidebar-label" style={{ marginTop: 8 }}>
        项目
      </div>
      {projects.length === 0 && <div className="sidebar-empty">暂无项目</div>}
      {projects.map((p) => {
        const active = inDetail && page.projectId === p.id;
        return (
          <div
            key={p.id}
            className={`sidebar-item ${active ? "active" : ""}`}
            onClick={() => onSelectProject(p.id)}
            title={p.base_path}
          >
            <span className={`item-dot ${active ? "active" : ""}`} />
            <span
              style={{
                flex: 1,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {p.name}
            </span>
            {(depthById[p.id] ?? 0) > 0 && (
              <span className="count-badge">{depthById[p.id]}</span>
            )}
          </div>
        );
      })}

      <div className="sidebar-actions">
        <div className="sidebar-item" onClick={onAddProject}>
          <span className="action-icon">＋</span>新建项目
        </div>
      </div>
    </div>
  );
}
