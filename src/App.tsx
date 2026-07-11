import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { ConnectionMeta, QueryResult } from "./api/types";

interface DeviceCode {
  user_code: string;
  verification_uri: string;
}
import { Sidebar } from "./components/Sidebar";
import { SchemaTree } from "./components/SchemaTree";
import { SqlEditor } from "./components/SqlEditor";
import { ResultsGrid } from "./components/ResultsGrid";
import { HistoryPanel } from "./components/HistoryPanel";
import { Settings } from "./components/Settings";
import { ConnectionWizard } from "./components/ConnectionWizard";

type Panel = "results" | "history" | "settings";

export default function App() {
  const [loggedIn, setLoggedIn] = useState(false);
  const [connections, setConnections] = useState<ConnectionMeta[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [connectedIds, setConnectedIds] = useState<Set<string>>(new Set());
  const [wizardOpen, setWizardOpen] = useState(false);
  const [panel, setPanel] = useState<Panel>("results");
  const [result, setResult] = useState<QueryResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deviceCode, setDeviceCode] = useState<DeviceCode | null>(null);

  const refreshConnections = useCallback(async () => {
    setConnections(await api.listSavedConnections());
  }, []);

  // While a login is pending, the Rust core emits the code the user must confirm in the browser.
  useEffect(() => {
    const unlisten = listen<DeviceCode>("sso-device-code", (e) => setDeviceCode(e.payload));
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const status = await api.ssoStatus();
        setLoggedIn(status.logged_in);
      } catch {
        /* offline / not logged in */
      }
      await refreshConnections();
    })();
  }, [refreshConnections]);

  const login = async () => {
    setError(null);
    try {
      const status = await api.ssoLogin();
      setLoggedIn(status.logged_in);
    } catch (e) {
      setError(String(e));
    } finally {
      setDeviceCode(null);
    }
  };

  const connect = async (meta: ConnectionMeta) => {
    setError(null);
    try {
      await api.connectDb(meta);
      setConnectedIds((s) => new Set(s).add(meta.id));
      setActiveId(meta.id);
    } catch (e) {
      setError(String(e));
    }
  };

  const disconnect = async (id: string) => {
    setError(null);
    try {
      await api.disconnectDb(id);
    } catch (e) {
      // Surface it, but still drop the connection from the UI — a failed backend close
      // shouldn't leave the row stuck showing "connected".
      setError(String(e));
    } finally {
      setConnectedIds((s) => {
        const n = new Set(s);
        n.delete(id);
        return n;
      });
      if (activeId === id) {
        setActiveId(null);
        setResult(null);
      }
    }
  };

  const runQuery = async (sql: string) => {
    if (!activeId) return;
    setRunning(true);
    setError(null);
    setPanel("results");
    try {
      setResult(await api.runQuery(activeId, sql));
    } catch (e) {
      setError(String(e));
      setResult(null);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="app">
      <header className="titlebar">
        <span className="brand">rdsconnect</span>
        <div className="titlebar-actions">
          <span className={`sso-dot ${loggedIn ? "on" : "off"}`} />
          <button onClick={login}>{loggedIn ? "SSO ✓" : "Log in"}</button>
          <button onClick={() => setPanel("history")}>History</button>
          <button onClick={() => setPanel("settings")}>Settings</button>
        </div>
      </header>

      {deviceCode && (
        <div className="device-code">
          Approve the sign-in in your browser. Confirm this code matches:
          <strong>{deviceCode.user_code}</strong>
          <span className="muted">
            (didn't open? visit {deviceCode.verification_uri})
          </span>
        </div>
      )}

      <div className="layout">
        <Sidebar
          connections={connections}
          activeId={activeId}
          connectedIds={connectedIds}
          onSelect={setActiveId}
          onConnect={connect}
          onDisconnect={disconnect}
          onNew={() => setWizardOpen(true)}
          onDelete={async (id) => {
            await api.deleteConnection(id);
            await refreshConnections();
          }}
        />

        {activeId && connectedIds.has(activeId) && (
          <SchemaTree connectionId={activeId} />
        )}

        <main className="workspace">
          <SqlEditor
            disabled={!activeId || !connectedIds.has(activeId)}
            running={running}
            onRun={runQuery}
          />
          {error && <div className="error-bar">{error}</div>}
          <div className="panel">
            {panel === "results" && <ResultsGrid result={result} />}
            {panel === "history" && <HistoryPanel />}
            {panel === "settings" && (
              <Settings onClose={() => setPanel("results")} />
            )}
          </div>
        </main>
      </div>

      {wizardOpen && (
        <ConnectionWizard
          loggedIn={loggedIn}
          onLogin={login}
          onClose={() => setWizardOpen(false)}
          onSaved={async () => {
            setWizardOpen(false);
            await refreshConnections();
          }}
        />
      )}
    </div>
  );
}
