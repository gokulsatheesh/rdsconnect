import { useEffect, useState } from "react";
import { api } from "../api";
import type { ColumnInfo } from "../api/types";

interface Props {
  connectionId: string;
  width: number;
}

/** databases -> tables -> columns, each level lazy-loaded on expand. */
export function SchemaTree({ connectionId, width }: Props) {
  const [schemas, setSchemas] = useState<string[]>([]);
  const [open, setOpen] = useState<Record<string, boolean>>({});

  useEffect(() => {
    api.listSchemas(connectionId).then(setSchemas).catch(() => setSchemas([]));
  }, [connectionId]);

  return (
    <nav className="schema-tree" style={{ width }}>
      <div className="schema-header">Schema</div>
      <ul>
        {schemas.map((s) => (
          <SchemaNode
            key={s}
            connectionId={connectionId}
            schema={s}
            open={!!open[s]}
            onToggle={() => setOpen((o) => ({ ...o, [s]: !o[s] }))}
          />
        ))}
      </ul>
    </nav>
  );
}

function SchemaNode({
  connectionId,
  schema,
  open,
  onToggle,
}: {
  connectionId: string;
  schema: string;
  open: boolean;
  onToggle: () => void;
}) {
  const [tables, setTables] = useState<string[] | null>(null);

  useEffect(() => {
    if (open && tables === null) {
      api.listTables(connectionId, schema).then(setTables).catch(() => setTables([]));
    }
  }, [open, tables, connectionId, schema]);

  return (
    <li className="tree-node">
      <div className="tree-row" onClick={onToggle}>
        <span className="twisty">{open ? "▾" : "▸"}</span>
        <span className="node-icon db" aria-hidden="true" />
        {schema}
      </div>
      {open && (
        <ul className="tree-children">
          {(tables ?? []).map((t) => (
            <TableNode
              key={t}
              connectionId={connectionId}
              schema={schema}
              table={t}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function TableNode({
  connectionId,
  schema,
  table,
}: {
  connectionId: string;
  schema: string;
  table: string;
}) {
  const [open, setOpen] = useState(false);
  const [cols, setCols] = useState<ColumnInfo[] | null>(null);

  useEffect(() => {
    if (open && cols === null) {
      api
        .listColumns(connectionId, schema, table)
        .then(setCols)
        .catch(() => setCols([]));
    }
  }, [open, cols, connectionId, schema, table]);

  return (
    <li className="tree-node">
      <div className="tree-row" onClick={() => setOpen((o) => !o)}>
        <span className="twisty">{open ? "▾" : "▸"}</span>
        <span className="node-icon tbl" aria-hidden="true" />
        {table}
      </div>
      {open && (
        <ul className="tree-children">
          {(cols ?? []).map((c) => (
            <li key={c.name} className="col-row">
              <span className={c.key === "PRI" ? "pk" : ""}>{c.name}</span>
              <span className="col-type">{c.data_type}</span>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}
