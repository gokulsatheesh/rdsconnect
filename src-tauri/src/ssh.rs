// SSH tunnel support for reaching a DB that's only accessible from inside the VPC (via a
// bastion / jump host).
//
// We shell out to the system OpenSSH client for local port-forwarding rather than reimplementing
// SSH in-process. This reuses the user's existing ~/.ssh/config, agent, keys, known_hosts, and
// ProxyJump chains — exactly what someone with a bastion already has set up — and keeps every SSH
// secret out of this process (agent + ssh handle auth). Consistent with the app's "no secrets on
// disk / in the core" posture.
//
// The tunnel forwards 127.0.0.1:<local port> -> <db host>:<db port> through the SSH connection;
// the MySQL client then connects to the local port. The child ssh process is held for the
// connection's lifetime and killed on teardown (also `kill_on_drop`, as a backstop).

use std::net::TcpListener;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::state::{AppError, AppResult, SshConfig};

/// A running SSH local-forward. Dropping it kills the ssh child (via `kill_on_drop`); callers
/// should prefer `close()` for a clean, awaited teardown.
pub struct SshTunnel {
    /// The 127.0.0.1 port the DB client should connect to.
    pub local_port: u16,
    child: Child,
}

impl std::fmt::Debug for SshTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshTunnel")
            .field("local_port", &self.local_port)
            .finish()
    }
}

impl SshTunnel {
    /// Open a tunnel forwarding a fresh local port to `db_host:db_port` through `ssh`.
    pub async fn open(ssh: &SshConfig, db_host: &str, db_port: u16) -> AppResult<Self> {
        let local_port = pick_local_port()?;

        let mut child = Command::new("ssh")
            .args(ssh_args(ssh, local_port, db_host, db_port))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                AppError::Db(format!(
                    "failed to start ssh (is the openssh client installed?): {e}"
                ))
            })?;

        // Wait until the forwarded port accepts connections — or ssh dies first, in which case
        // surface its stderr (auth failure, unknown host key, unreachable bastion, …).
        match wait_for_port(local_port, &mut child).await {
            Ok(()) => Ok(Self { local_port, child }),
            Err(e) => {
                let _ = child.start_kill();
                Err(e)
            }
        }
    }

    /// Kill the ssh child and wait for it to exit.
    pub async fn close(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

/// Reserve a free ephemeral port on loopback. There's an inherent TOCTOU race between dropping
/// the listener and ssh binding it, but the window is tiny and ssh's `ExitOnForwardFailure`
/// turns any collision into a clean, surfaced error rather than a silent hang.
fn pick_local_port() -> AppResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AppError::Db(format!("could not reserve a local tunnel port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::Db(e.to_string()))?
        .port();
    drop(listener);
    Ok(port)
}

/// Build the `ssh` argument vector for a non-interactive local-forward. Pure/deterministic so it
/// can be unit-tested. `BatchMode=yes` means only non-interactive auth (agent/key) is used — no
/// password prompts, since we have no TTY to answer them.
fn ssh_args(ssh: &SshConfig, local_port: u16, db_host: &str, db_port: u16) -> Vec<String> {
    vec![
        "-N".into(), // no remote command, just forward
        "-T".into(), // no TTY
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "ServerAliveInterval=30".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-p".into(),
        ssh.port.to_string(),
        "-L".into(),
        format!("127.0.0.1:{local_port}:{db_host}:{db_port}"),
        format!("{}@{}", ssh.user, ssh.host),
    ]
}

/// How long to wait for the SSH forward to come up before giving up.
const TUNNEL_READY_TIMEOUT: Duration = Duration::from_secs(15);

/// Poll the local port until it accepts a connection, or the ssh child exits (surfacing its
/// stderr), or we hit the timeout.
async fn wait_for_port(local_port: u16, child: &mut Child) -> AppResult<()> {
    let addr = format!("127.0.0.1:{local_port}");
    let deadline = tokio::time::Instant::now() + TUNNEL_READY_TIMEOUT;

    loop {
        // Did ssh exit already? If so, its stderr explains why (bad host key, auth, etc.).
        if let Ok(Some(status)) = child.try_wait() {
            let stderr = drain_stderr(child).await;
            let detail = stderr.trim();
            let detail = if detail.is_empty() {
                format!("ssh exited with {status}")
            } else {
                detail.to_string()
            };
            return Err(AppError::Db(format!("SSH tunnel failed: {detail}")));
        }

        if tokio::time::timeout(
            Duration::from_millis(300),
            tokio::net::TcpStream::connect(&addr),
        )
        .await
        .is_ok_and(|r| r.is_ok())
        {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Db(format!(
                "SSH tunnel did not come up within {}s — check the bastion host, user, and that \
                 your SSH key/agent can reach it",
                TUNNEL_READY_TIMEOUT.as_secs()
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Best-effort read of whatever the ssh child wrote to stderr.
async fn drain_stderr(child: &mut Child) -> String {
    let mut buf = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut buf).await;
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SshConfig {
        SshConfig {
            host: "bastion.example.com".into(),
            user: "gokul".into(),
            port: 22,
        }
    }

    #[test]
    fn ssh_args_build_a_non_interactive_local_forward() {
        let args = ssh_args(&cfg(), 54321, "db.internal.rds.amazonaws.com", 3306);
        // The forward maps the local port to the real DB endpoint.
        assert!(args.contains(&"-L".to_string()));
        assert!(args.contains(&"127.0.0.1:54321:db.internal.rds.amazonaws.com:3306".to_string()));
        // Non-interactive + fail-fast forwarding.
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_string()));
        // user@host target and the SSH port.
        assert!(args.contains(&"gokul@bastion.example.com".to_string()));
        assert!(args.contains(&"22".to_string()));
        // No remote command.
        assert!(args.contains(&"-N".to_string()));
    }

    #[test]
    fn ssh_args_honor_a_custom_ssh_port() {
        let mut c = cfg();
        c.port = 2222;
        let args = ssh_args(&c, 5000, "h", 3306);
        let p = args.iter().position(|a| a == "-p").expect("has -p");
        assert_eq!(args[p + 1], "2222");
    }

    #[test]
    fn pick_local_port_returns_a_usable_port() {
        let port = pick_local_port().expect("port");
        assert!(port > 0);
    }
}
