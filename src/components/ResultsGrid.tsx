import { useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { api } from "../api";
import type { QueryResult } from "../api/types";

interface Props {
  result: QueryResult | null;
}

/** Virtualized results table with CSV export. Handles large result sets. */
export function ResultsGrid({ result }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const rows = result?.rows ?? [];
  const [exporting, setExporting] = useState(false);
  const [exportMsg, setExportMsg] = useState<string | null>(null);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 28,
    overscan: 12,
  });

  const csv = useMemo(() => (result ? toCsv(result) : ""), [result]);

  const onExport = async () => {
    setExporting(true);
    setExportMsg(null);
    try {
      const saved = await api.exportCsv(`rdsconnect-results-${Date.now()}.csv`, csv);
      setExportMsg(saved ? `Saved to ${saved}` : null); // null = user cancelled
    } catch (e) {
      setExportMsg(`Export failed: ${String(e)}`);
    } finally {
      setExporting(false);
    }
  };

  if (!result) {
    return <div className="results-empty">Run a query to see results.</div>;
  }

  return (
    <div className="results">
      <div className="results-toolbar">
        <span>
          {result.rows_affected} row{result.rows_affected === 1 ? "" : "s"} ·{" "}
          {result.elapsed_ms} ms
        </span>
        {exportMsg && <span className="export-msg muted">{exportMsg}</span>}
        <button disabled={rows.length === 0 || exporting} onClick={onExport}>
          {exporting ? "Saving…" : "Export CSV"}
        </button>
      </div>

      <div className="grid-scroll" ref={parentRef}>
        {/* A CSS-grid table, not an HTML <table>: virtualized rows are absolutely positioned,
            which a real <table> layout ignores (rows collapse/overlap). Header and body rows
            share the same grid-template-columns so columns stay aligned. */}
        <div className="dgrid">
          <div
            className="dgrid-head"
            style={{ gridTemplateColumns: gridTemplate(result.columns.length) }}
          >
            <div className="dcell rownum" />
            {result.columns.map((c) => (
              <div className="dcell dhead" key={c} title={c}>
                {c}
              </div>
            ))}
          </div>
          <div
            className="dgrid-body"
            style={{ height: rowVirtualizer.getTotalSize() }}
          >
            {rowVirtualizer.getVirtualItems().map((vi) => {
              const row = rows[vi.index];
              return (
                <div
                  key={vi.key}
                  className="dgrid-row"
                  style={{
                    transform: `translateY(${vi.start}px)`,
                    height: vi.size,
                    gridTemplateColumns: gridTemplate(result.columns.length),
                  }}
                >
                  <div className="dcell rownum">{vi.index + 1}</div>
                  {row.map((cell, ci) => (
                    <div
                      key={ci}
                      className={`dcell ${cell === null ? "null-cell" : ""}`}
                      title={cell ?? "NULL"}
                    >
                      {cell === null ? "NULL" : cell}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

/** Row-number gutter (48px) + one flexible track per column (min 140px, grows to fill). */
function gridTemplate(columns: number): string {
  return `48px repeat(${columns}, minmax(140px, 1fr))`;
}

function toCsv(result: QueryResult): string {
  const esc = (v: string | null) => {
    if (v === null) return "";
    // Guard against CSV formula injection: a cell starting with = + - @ (or a
    // leading control char) is treated as a formula by spreadsheet apps. Prefix
    // with a single quote so it's rendered as literal text.
    let s = v;
    if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`;
    const needsQuote = /[",\n]/.test(s);
    s = s.replace(/"/g, '""');
    return needsQuote ? `"${s}"` : s;
  };
  const header = result.columns.map(esc).join(",");
  const body = result.rows.map((r) => r.map(esc).join(",")).join("\n");
  return `${header}\n${body}`;
}
