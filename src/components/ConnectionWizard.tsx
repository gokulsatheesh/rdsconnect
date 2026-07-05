import { useEffect, useState } from "react";
import { api } from "../api";
import type { Account, DbInstance } from "../api/types";

interface Props {
  loggedIn: boolean;
  onLogin: () => void;
  onClose: () => void;
  onSaved: () => void;
}

const REGIONS = [
  "us-east-1",
  "us-east-2",
  "us-west-1",
  "us-west-2",
  "ap-south-1",
  "ap-southeast-1",
  "eu-west-1",
];

/** Walks the exact rds-connect.sh sequence as GUI steps, then saves a connection. */
export function ConnectionWizard({ loggedIn, onLogin, onClose, onSaved }: Props) {
  const [step, setStep] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [account, setAccount] = useState<Account | null>(null);
  const [roles, setRoles] = useState<string[]>([]);
  const [role, setRole] = useState<string | null>(null);
  const [region, setRegion] = useState("us-east-1");
  const [instances, setInstances] = useState<DbInstance[]>([]);
  const [instance, setInstance] = useState<DbInstance | null>(null);
  const [dbUser, setDbUser] = useState("");
  const [label, setLabel] = useState("");

  useEffect(() => {
    api.getSettings().then((s) => {
      setDbUser(s.default_db_user);
      setRegion(s.default_rds_region);
    });
  }, []);

  const guard = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toAccounts = () =>
    guard(async () => {
      setAccounts(await api.listAccounts());
      setStep(1);
    });

  const pickAccount = (a: Account) =>
    guard(async () => {
      setAccount(a);
      const r = await api.listAccountRoles(a.account_id);
      setRoles(r);
      // Auto-select if exactly one role, mirroring the script.
      if (r.length === 1) {
        setRole(r[0]);
        setStep(3);
      } else {
        setStep(2);
      }
    });

  const pickRole = (r: string) => {
    setRole(r);
    setStep(3);
  };

  const loadInstances = () =>
    guard(async () => {
      if (!account || !role) return;
      const list = await api.listDbInstances(account.account_id, role, region);
      setInstances(list);
      if (list.length === 1) {
        setInstance(list[0]);
        setStep(5);
      } else {
        setStep(4);
      }
    });

  const save = () =>
    guard(async () => {
      if (!account || !role || !instance) return;
      const id = `${account.account_id}:${instance.identifier}:${dbUser}`;
      await api.saveConnection({
        id,
        label: label || `${instance.identifier} (${account.account_name})`,
        account_id: account.account_id,
        role_name: role,
        region,
        instance_id: instance.identifier,
        host: instance.host,
        port: instance.port,
        db_user: dbUser,
        default_database: null,
      });
      onSaved();
    });

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wizard" onClick={(e) => e.stopPropagation()}>
        <div className="wizard-steps">
          {["SSO", "Account", "Role", "Region", "Instance", "Save"].map((s, i) => (
            <span key={s} className={`wstep ${i === step ? "active" : ""} ${i < step ? "done" : ""}`}>
              {s}
            </span>
          ))}
        </div>

        {error && <div className="error-bar">{error}</div>}

        <div className="wizard-body">
          {step === 0 && (
            <div className="wcol">
              <p>Sign in with AWS SSO to discover accounts and databases.</p>
              {loggedIn ? (
                <button onClick={toAccounts} disabled={busy}>
                  Continue →
                </button>
              ) : (
                <button onClick={onLogin}>Log in with SSO</button>
              )}
            </div>
          )}

          {step === 1 && (
            <ul className="pick-list">
              {accounts.map((a) => (
                <li key={a.account_id} onClick={() => pickAccount(a)}>
                  <strong>{a.account_name}</strong>
                  <span className="muted">{a.account_id}</span>
                </li>
              ))}
            </ul>
          )}

          {step === 2 && (
            <ul className="pick-list">
              {roles.map((r) => (
                <li key={r} onClick={() => pickRole(r)}>
                  {r}
                </li>
              ))}
            </ul>
          )}

          {step === 3 && (
            <div className="wcol">
              <label>
                RDS region
                <input
                  list="regions"
                  value={region}
                  onChange={(e) => setRegion(e.target.value)}
                />
                <datalist id="regions">
                  {REGIONS.map((r) => (
                    <option key={r} value={r} />
                  ))}
                </datalist>
              </label>
              <button onClick={loadInstances} disabled={busy}>
                Find instances →
              </button>
            </div>
          )}

          {step === 4 && (
            <ul className="pick-list">
              {instances.map((i) => (
                <li
                  key={i.identifier}
                  onClick={() => {
                    setInstance(i);
                    setStep(5);
                  }}
                >
                  <strong>{i.identifier}</strong>
                  <span className="muted">
                    {i.host}:{i.port} · {i.engine}
                  </span>
                </li>
              ))}
            </ul>
          )}

          {step === 5 && instance && (
            <div className="wcol">
              <label>
                Label
                <input value={label} onChange={(e) => setLabel(e.target.value)} placeholder={instance.identifier} />
              </label>
              <label>
                DB user
                <input value={dbUser} onChange={(e) => setDbUser(e.target.value)} />
              </label>
              <div className="summary muted">
                {instance.host}:{instance.port} · {region} · {account?.account_name} / {role}
              </div>
              <button onClick={save} disabled={busy}>
                Save connection
              </button>
            </div>
          )}
        </div>

        <div className="wizard-footer">
          <button className="ghost" onClick={onClose}>
            Cancel
          </button>
          {step > 0 && (
            <button className="ghost" onClick={() => setStep((s) => Math.max(0, s - 1))}>
              ← Back
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
