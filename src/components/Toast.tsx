export interface ToastItem {
  id: number;
  message: string;
  hint?: string;
  kind: "success" | "error";
}

export default function Toast({ toasts }: { toasts: ToastItem[] }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-wrap" data-testid="toast-wrap">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind === "error" ? "error" : ""}`}>
          {t.message}
          {t.hint && <span className="toast-hint">{t.hint}</span>}
        </div>
      ))}
    </div>
  );
}
