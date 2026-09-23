export default function StatusBar({
  left,
  right,
}: {
  left: string;
  right?: string;
}) {
  return (
    <div className="status-bar" data-testid="status-bar">
      <span>{left}</span>
      {right && <span>{right}</span>}
    </div>
  );
}
