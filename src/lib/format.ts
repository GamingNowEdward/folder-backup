import type { LayerStatus } from "./types";

export function statusLabel(status: LayerStatus | string): string {
  switch (status) {
    case "creating":
      return "创建中";
    case "backed_up":
      return "已备份";
    case "applied":
      return "已应用";
    case "restoring":
      return "恢复中";
    case "rolled_back":
      return "已恢复";
    default:
      return status;
  }
}

export function formatTime(iso: string): string {
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  } catch {
    return iso;
  }
}
