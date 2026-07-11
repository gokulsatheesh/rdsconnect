// Types mirroring the Rust command signatures (src-tauri/src/*).

export interface Account {
  account_id: string;
  account_name: string;
  email: string | null;
}

export interface DbInstance {
  identifier: string;
  host: string;
  port: number;
  engine: string;
  region: string;
}

/** Optional SSH jump-host config. No secrets — auth is delegated to the system ssh client
 * (agent, keys, ~/.ssh/config, known_hosts). Mirrors Rust `SshConfig`. */
export interface SshConfig {
  host: string;
  user: string;
  port: number;
}

/** Everything needed to (re)build a connection. Mirrors Rust `ConnectionMeta`. No secrets. */
export interface ConnectionMeta {
  id: string;
  label: string;
  account_id: string;
  role_name: string;
  region: string;
  instance_id: string;
  host: string;
  port: number;
  db_user: string;
  default_database?: string | null;
  ssh?: SshConfig | null;
}

export interface QueryResult {
  columns: string[];
  rows: (string | null)[][];
  rows_affected: number;
  elapsed_ms: number;
}

export interface ColumnInfo {
  name: string;
  data_type: string;
  nullable: boolean;
  key: string;
}

export interface HistoryEntry {
  connection_id: string;
  sql: string;
  at: string;
  rows: number;
  elapsed_ms: number;
  ok: boolean;
}

export interface Settings {
  sso_start_url: string;
  sso_region: string;
  default_db_user: string;
  default_rds_region: string;
}

export interface SsoStatus {
  logged_in: boolean;
}
