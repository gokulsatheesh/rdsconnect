import { useEffect, useState } from "react";
import { api } from "../api";
import type { Settings as SettingsType } from "../api/types";

interface Props {
  onClose: () => void;
}

export function Settings({ onClose }: Props) {
  const [settings, setSettings] = useState<SettingsType | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.getSettings().then(setSettings);
  }, []);

  if (!settings) return <div className="results-empty">Loading settings…</div>;

  const update = (patch: Partial<SettingsType>) =>
    setSettings((s) => (s ? { ...s, ...patch } : s));

  const save = async () => {
    await api.updateSettings(settings);
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  return (
    <div className="settings">
      <h2>Settings</h2>
      <label>
        SSO start URL
        <input
          value={settings.sso_start_url}
          onChange={(e) => update({ sso_start_url: e.target.value })}
        />
      </label>
      <label>
        SSO region
        <input
          value={settings.sso_region}
          onChange={(e) => update({ sso_region: e.target.value })}
        />
      </label>
      <label>
        Default DB user
        <input
          value={settings.default_db_user}
          onChange={(e) => update({ default_db_user: e.target.value })}
        />
      </label>
      <label>
        Default RDS region
        <input
          value={settings.default_rds_region}
          onChange={(e) => update({ default_rds_region: e.target.value })}
        />
      </label>
      <div className="settings-actions">
        <button onClick={save}>{saved ? "Saved ✓" : "Save"}</button>
        <button className="ghost" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
