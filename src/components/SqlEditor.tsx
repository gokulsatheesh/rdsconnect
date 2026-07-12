import { useEffect, useRef, useState } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap } from "@codemirror/commands";
import { sql, MySQL } from "@codemirror/lang-sql";

interface Props {
  disabled: boolean;
  running: boolean;
  onRun: (sql: string) => void;
  /** When this changes, replace the active tab's content with `sql` (e.g. from history). */
  loadSql: { sql: string; n: number } | null;
}

interface Tab {
  id: number;
  title: string;
  doc: string;
}

let nextId = 1;

/** Multi-tab CodeMirror 6 editor. Cmd/Ctrl+Enter runs the current tab. */
export function SqlEditor({ disabled, running, onRun, loadSql }: Props) {
  const [tabs, setTabs] = useState<Tab[]>([
    { id: nextId++, title: "Query 1", doc: "SELECT 1;" },
  ]);
  const [activeTab, setActiveTab] = useState(tabs[0].id);
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onRunRef = useRef(onRun);
  onRunRef.current = onRun;

  // (Re)build the editor when the active tab changes.
  useEffect(() => {
    if (!hostRef.current) return;
    const tab = tabs.find((t) => t.id === activeTab);
    if (!tab) return;

    const runKeymap = keymap.of([
      {
        key: "Mod-Enter",
        run: (v) => {
          onRunRef.current(v.state.doc.toString());
          return true;
        },
      },
    ]);

    const state = EditorState.create({
      doc: tab.doc,
      extensions: [
        keymap.of(defaultKeymap),
        runKeymap,
        sql({ dialect: MySQL }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            const doc = u.state.doc.toString();
            setTabs((ts) =>
              ts.map((t) => (t.id === activeTab ? { ...t, doc } : t)),
            );
          }
        }),
      ],
    });
    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;
    return () => view.destroy();
    // Intentionally only re-create on tab switch, not on every doc change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab]);

  // Inject SQL from an external source (history) into the current editor, replacing its content.
  // The updateListener keeps the tab's `doc` in sync, so no extra setTabs is needed.
  useEffect(() => {
    if (!loadSql) return;
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: loadSql.sql },
    });
    view.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadSql?.n]);

  const run = () => {
    const doc = viewRef.current?.state.doc.toString() ?? "";
    onRun(doc);
  };

  const addTab = () => {
    const t = { id: nextId++, title: `Query ${tabs.length + 1}`, doc: "" };
    setTabs((ts) => [...ts, t]);
    setActiveTab(t.id);
  };

  const closeTab = (id: number) => {
    setTabs((ts) => {
      const next = ts.filter((t) => t.id !== id);
      if (next.length === 0) {
        const fresh = { id: nextId++, title: "Query 1", doc: "" };
        setActiveTab(fresh.id);
        return [fresh];
      }
      if (id === activeTab) setActiveTab(next[0].id);
      return next;
    });
  };

  return (
    <section className="editor">
      <div className="tabs">
        {tabs.map((t) => (
          <div
            key={t.id}
            className={`tab ${t.id === activeTab ? "active" : ""}`}
            onClick={() => setActiveTab(t.id)}
          >
            {t.title}
            <span
              className="tab-close"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(t.id);
              }}
            >
              ×
            </span>
          </div>
        ))}
        <button className="tab-add" onClick={addTab}>
          +
        </button>
        <div className="editor-run">
          <button disabled={disabled || running} onClick={run}>
            {running ? "Running…" : "Run ▷ (⌘↵)"}
          </button>
        </div>
      </div>
      <div ref={hostRef} className="cm-host" />
    </section>
  );
}
