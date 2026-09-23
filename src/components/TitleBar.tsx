import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

function onDragMouseDown(e: React.MouseEvent): void {
  if ((e.target as HTMLElement).closest(".title-bar-buttons")) return;
  const startX = e.clientX;
  const startY = e.clientY;
  let dragged = false;

  const onMove = (me: MouseEvent): void => {
    if (
      !dragged &&
      (Math.abs(me.clientX - startX) > 5 || Math.abs(me.clientY - startY) > 5)
    ) {
      dragged = true;
      getCurrentWindow().startDragging().catch((err: unknown) => {
        console.error("[TitleBar] startDragging 失败:", err);
      });
    }
  };
  const onUp = (): void => {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  };
  document.addEventListener("mousemove", onMove);
  document.addEventListener("mouseup", onUp);
}

export default function TitleBar() {
  const dragRef = useRef(onDragMouseDown);
  useEffect(() => undefined, []);

  return (
    <div
      className="title-bar"
      data-testid="title-bar"
      onMouseDown={(e) => dragRef.current(e)}
      onDoubleClick={() => void getCurrentWindow().toggleMaximize()}
    >
      <div className="title-bar-left">
        <span className="title-icon">📁</span>
        <span className="title-text">Folder Backup</span>
      </div>
      <div className="title-bar-buttons" onDoubleClick={(e) => e.stopPropagation()}>
        <button
          className="win-btn"
          title="最小化"
          onClick={() => void getCurrentWindow().minimize()}
        >
          <svg width="16" height="16" viewBox="0 0 16 16">
            <line x1="3" y1="8" x2="13" y2="8" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </button>
        <button
          className="win-btn"
          title="最大化"
          onClick={() => void getCurrentWindow().toggleMaximize()}
        >
          <svg width="16" height="16" viewBox="0 0 16 16">
            <rect x="3.5" y="3.5" width="9" height="9" rx="1.5" fill="none" stroke="currentColor" strokeWidth="1.2" />
          </svg>
        </button>
        <button
          className="win-btn win-close"
          title="关闭"
          onClick={() => void getCurrentWindow().close()}
        >
          <svg width="16" height="16" viewBox="0 0 16 16">
            <line x1="4" y1="4" x2="12" y2="12" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <line x1="12" y1="4" x2="4" y2="12" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </div>
  );
}
