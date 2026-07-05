import { useEffect, useState } from "react";
import { api } from "../api";
import type { HistoryEntry } from "../api/types";

export function HistoryPanel() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);

  useEffect(() => {
    api.listHistory().then((h) => setEntries([...h].reverse())).catch(() => setEntries([]));
  }, []);

  if (entries.length === 0) {
    return <div className="results-empty">No query history yet.</div>;
  }

  return (
    <div className="history">
      <table className="grid">
        <thead>
          <tr>
            <th>When</th>
            <th>Rows</th>
            <th>Time</th>
            <th>SQL</th>
          </tr>
        </thead>
        <tbody>
          {entries.map((e, i) => (
            <tr key={i} className={e.ok ? "" : "row-error"}>
              <td>{new Date(e.at).toLocaleString()}</td>
              <td>{e.rows}</td>
              <td>{e.elapsed_ms} ms</td>
              <td className="sql-cell">{e.sql}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
