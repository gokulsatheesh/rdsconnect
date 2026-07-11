// One typed function per Rust command. The UI imports only from here — it never calls
// `invoke` directly, so command names and payload shapes live in a single place.

import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  ColumnInfo,
  ConnectionMeta,
  DbInstance,
  HistoryEntry,
  QueryResult,
  Settings,
  SsoStatus,
} from "./types";

export const api = {
  ping: () => invoke<string>("ping"),

  // auth
  ssoStatus: () => invoke<SsoStatus>("sso_status"),
  ssoLogin: () => invoke<SsoStatus>("sso_login"),
  listAccounts: () => invoke<Account[]>("list_accounts"),
  listAccountRoles: (accountId: string) =>
    invoke<string[]>("list_account_roles", { accountId }),
  listDbInstances: (accountId: string, roleName: string, region: string) =>
    invoke<DbInstance[]>("list_db_instances", { accountId, roleName, region }),

  // db
  connectDb: (meta: ConnectionMeta) => invoke<void>("connect_db", { meta }),
  disconnectDb: (id: string) => invoke<void>("disconnect_db", { id }),
  runQuery: (id: string, sql: string) =>
    invoke<QueryResult>("run_query", { id, sql }),
  listSchemas: (id: string) => invoke<string[]>("list_schemas", { id }),
  listTables: (id: string, schema: string) =>
    invoke<string[]>("list_tables", { id, schema }),
  listColumns: (id: string, schema: string, table: string) =>
    invoke<ColumnInfo[]>("list_columns", { id, schema, table }),
  /** Save CSV via a native dialog. Resolves to the saved path, or null if cancelled. */
  exportCsv: (defaultName: string, content: string) =>
    invoke<string | null>("export_csv", { defaultName, content }),

  // persistence
  listSavedConnections: () =>
    invoke<ConnectionMeta[]>("list_saved_connections"),
  saveConnection: (meta: ConnectionMeta) =>
    invoke<ConnectionMeta[]>("save_connection", { meta }),
  deleteConnection: (id: string) =>
    invoke<ConnectionMeta[]>("delete_connection", { id }),
  listHistory: () => invoke<HistoryEntry[]>("list_history"),
  appendHistory: (entry: HistoryEntry) =>
    invoke<void>("append_history", { entry }),

  // settings
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) =>
    invoke<Settings>("update_settings", { settings }),
};

export type { Account, ColumnInfo, ConnectionMeta, DbInstance, HistoryEntry, QueryResult, Settings, SsoStatus };
