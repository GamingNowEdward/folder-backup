import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getSettings, setSettings } from "../lib/ipc";

export default function SettingsPage({ onSaved }: { onSaved: (root: string) => void }) {
  const [root, setRoot] = useState("");
  const [defaultRoot, setDefaultRoot] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void getSettings().then((s) => {
      setRoot(s.backup_root);
      setDefaultRoot(s.backup_root);
      setLoading(false);
    });
  }, []);

  async function browse(): Promise<void> {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setRoot(picked);
  }

  async function save(): Promise<void> {
    setSaving(true);
    try {
      const s = await setSettings(root);
      setRoot(s.backup_root);
      setDefaultRoot(s.backup_root);
      onSaved(s.backup_root);
    } finally {
      setSaving(false);
    }
  }

  // header 常驻：加载态只替换内容区，避免切换页面时标题闪没/跳动
  return (
    <div className="page" data-testid="settings-page">
      <div className="main-header">
        <div className="main-title">设置</div>
      </div>
      <div className="content-scroll">
        {loading ? (
          <div className="empty-hint" data-testid="settings-loading">
            加载中…
          </div>
        ) : (
          <div className="settings-form">
            <div className="settings-block">
              <div className="settings-block-title">备份根目录</div>
              <div className="settings-block-desc">
                所有项目的层备份（meta.json + files/）存放于此。默认
                %LOCALAPPDATA%\FolderBackup\backups。
              </div>
              <div className="field-row">
                <input
                  className="dialog-input"
                  value={root}
                  data-testid="settings-root-input"
                  onChange={(e) => setRoot(e.target.value)}
                />
                <button className="btn-browse" onClick={() => void browse()}>
                  浏览…
                </button>
              </div>
              <div className="settings-actions">
                <button
                  className="btn btn-accent"
                  onClick={() => void save()}
                  disabled={saving || !root.trim()}
                  data-testid="settings-save"
                >
                  {saving ? "保存中…" : "保存"}
                </button>
                <button className="btn" onClick={() => setRoot(defaultRoot)}>
                  还原
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
