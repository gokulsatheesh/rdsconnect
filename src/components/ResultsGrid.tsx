import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { QueryResult } from "../api/types";

interface Props {
  result: QueryResult | null;
}

/** Virtualized results table with CSV export. Handles large result sets. */
export function ResultsGrid({ result }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const rows = result?.rows ?? [];

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 28,
    overscan: 12,
  });

  const csv = useMemo(() => (result ? toCsv(result) : ""), [result]);

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
        <button
          disabled={rows.length === 0}
          onClick={() => downloadCsv(csv)}
        >
          Export CSV
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

function downloadCsv(csv: string) {
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `bifrost-results-${Date.now()}.csv`;
  a.click();
  URL.revokeObjectURL(url);
}
