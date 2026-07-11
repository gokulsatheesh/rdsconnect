import type { ConnectionMeta } from "../api/types";

interface Props {
  connections: ConnectionMeta[];
  activeId: string | null;
  connectedIds: Set<string>;
  onSelect: (id: string) => void;
  onConnect: (meta: ConnectionMeta) => void;
  onDisconnect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
}

export function Sidebar({
  connections,
  activeId,
  connectedIds,
  onSelect,
  onConnect,
  onDisconnect,
  onNew,
  onDelete,
}: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span>Connections</span>
        <button className="ghost" onClick={onNew} title="New connection">
          +
        </button>
      </div>
      <ul className="conn-list">
        {connections.length === 0 && (
          <li className="empty">No saved connections yet.</li>
        )}
        {connections.map((c) => {
          const connected = connectedIds.has(c.id);
          return (
            <li
              key={c.id}
              className={`conn-item ${activeId === c.id ? "active" : ""}`}
              onClick={() => onSelect(c.id)}
            >
              <div className="conn-main">
                <span className={`status-dot ${connected ? "on" : "off"}`} />
                <div>
                  <div className="conn-label">
                    {c.label}
                    {c.ssh && (
                      <span className="ssh-badge" title={`via ssh ${c.ssh.user}@${c.ssh.host}`}>
                        SSH
                      </span>
                    )}
                  </div>
                  <div className="conn-sub">
                    {c.instance_id} · {c.region} · {c.db_user}
                  </div>
                </div>
              </div>
              <div className="conn-actions">
                {connected ? (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onDisconnect(c.id);
                    }}
                  >
                    Disconnect
                  </button>
                ) : (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onConnect(c);
                    }}
                  >
                    Connect
                  </button>
                )}
                <button
                  className="ghost danger"
                  title="Delete"
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(c.id);
                  }}
                >
                  ×
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}
