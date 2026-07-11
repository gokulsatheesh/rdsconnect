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
        <table className="grid">
          <thead>
            <tr>
              <th className="rownum" />
              {result.columns.map((c) => (
                <th key={c}>{c}</th>
              ))}
            </tr>
          </thead>
          <tbody
            style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}
          >
            {rowVirtualizer.getVirtualItems().map((vi) => {
              const row = rows[vi.index];
              return (
                <tr
                  key={vi.key}
                  style={{
                    position: "absolute",
                    top: 0,
                    transform: `translateY(${vi.start}px)`,
                    height: vi.size,
                  }}
                >
                  <td className="rownum">{vi.index + 1}</td>
                  {row.map((cell, ci) => (
                    <td key={ci} className={cell === null ? "null-cell" : ""}>
                      {cell === null ? "NULL" : cell}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
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
