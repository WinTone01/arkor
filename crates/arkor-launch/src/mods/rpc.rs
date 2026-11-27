//! Talking to Amethyst's headless service: JSON-RPC 2.0, one JSON value per line over its
//! standard input and output (`AppImage headless --stdio`, protocol 1). Long operations are
//! jobs: a `*.start` method answers with a job at once, and `jobs.get` is asked until it
//! ends. What Amethyst writes to standard error goes to a log of its own.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{Value, json};

/// The protocol Arkor speaks.
pub const PROTOCOL: u64 = 1;

/// How long one answer may take (jobs are asked about many times, each answering quickly).
const ANSWER: Duration = Duration::from_secs(120);

/// An error Amethyst answered with: its JSON-RPC code (0 for a job that failed), its own
/// short name for it (e.g. `mod_not_found`), what it said and its details.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub name: String,
    pub message: String,
    pub details: Value,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RpcError {}

impl RpcError {
    /// A JSON-RPC error (`data.name`, `data.details`) or a failed job's (`name`, `details`).
    fn from_value(v: &Value) -> RpcError {
        let field = |k: &str| {
            if v["data"].is_object() {
                v["data"][k].clone()
            } else {
                v[k].clone()
            }
        };
        RpcError {
            code: v["code"].as_i64().unwrap_or(0),
            name: field("name").as_str().unwrap_or_default().to_string(),
            message: v["message"]
                .as_str()
                .unwrap_or("Amethyst reported an error")
                .to_string(),
            details: field("details"),
        }
    }
}

/// A running headless Amethyst.
pub struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next: u64,
    /// Amethyst's own version, from the handshake.
    pub version: String,
}

impl Client {
    /// Starts `cmd` (Amethyst's headless service) and greets it.
    pub fn spawn(mut cmd: Command) -> anyhow::Result<Client> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("starting Amethyst")?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("no output from Amethyst")?;
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut client = Client {
            child,
            stdin,
            lines,
            next: 0,
            version: String::new(),
        };
        let hello = client.call(
            "system.handshake",
            json!({
                "protocol_version": PROTOCOL,
                "client": {"name": "Arkor", "version": crate::self_update::current_version()},
            }),
        )?;
        let (min, max) = (
            hello["protocol"]["minimum_version"].as_u64().unwrap_or(0),
            hello["protocol"]["maximum_version"].as_u64().unwrap_or(0),
        );
        anyhow::ensure!(
            (min..=max).contains(&PROTOCOL),
            "this Amethyst speaks protocol {min}–{max}, Arkor speaks {PROTOCOL}"
        );
        client.version = hello["application"]["version"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        Ok(client)
    }

    /// Whether it still runs.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Asks `method` and waits for its answer.
    pub fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next += 1;
        let id = self.next;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.stdin.as_mut().context("Amethyst is closed")?;
        writeln!(stdin, "{line}").context("Amethyst stopped")?;
        stdin.flush().context("Amethyst stopped")?;
        let until = Instant::now() + ANSWER;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(left) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => anyhow::bail!("Amethyst did not answer {method}"),
                Err(RecvTimeoutError::Disconnected) => anyhow::bail!("Amethyst stopped"),
            };
            let Ok(answer) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // Only answers come on this channel; one to an earlier, given-up request is skipped.
            if answer["id"] != json!(id) {
                continue;
            }
            if answer.get("error").is_some_and(|e| !e.is_null()) {
                return Err(RpcError::from_value(&answer["error"]).into());
            }
            return Ok(answer["result"].clone());
        }
    }

    /// Starts a job (`method` ends in `.start`) and waits until it ends; its result.
    /// `progress` hears every snapshot of it while it runs.
    pub fn job(
        &mut self,
        method: &str,
        params: Value,
        mut progress: impl FnMut(&Value),
    ) -> anyhow::Result<Value> {
        let started = self.call(method, params)?;
        let id = started["job"]["id"]
            .as_str()
            .context("Amethyst started no job")?
            .to_string();
        loop {
            let job = self.call("jobs.get", json!({"job_id": id}))?["job"].clone();
            match job["state"].as_str().unwrap_or_default() {
                "succeeded" => return Ok(job["result"].clone()),
                "failed" | "cancelled" | "interrupted" => {
                    let error = &job["error"];
                    return Err(if error.is_object() {
                        RpcError::from_value(error).into()
                    } else {
                        anyhow::anyhow!("{method} did not finish")
                    });
                }
                _ => progress(&job),
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

impl Drop for Client {
    /// Closing its input ends the service once its accepted jobs are done.
    fn drop(&mut self) {
        drop(self.stdin.take());
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for Amethyst: answers the handshake, a job that runs once and succeeds,
    /// and an error, by the request's method.
    fn fake(script: &str) -> Command {
        let mut cmd = Command::new("python3");
        cmd.args(["-I", "-c", script]);
        cmd
    }

    const SERVER: &str = r#"
import json, sys
seen = 0
for line in sys.stdin:
    r = json.loads(line); m = r["method"]; out = {"jsonrpc": "2.0", "id": r["id"]}
    if m == "system.handshake":
        out["result"] = {"protocol": {"minimum_version": 1, "maximum_version": 1}, "application": {"version": "9.9"}}
    elif m == "work.start":
        out["result"] = {"job": {"id": "j1", "state": "queued"}}
    elif m == "jobs.get":
        seen += 1
        state = "running" if seen < 2 else "succeeded"
        out["result"] = {"job": {"id": "j1", "state": state, "result": {"done": True} if state == "succeeded" else None}}
    elif m == "broken.start":
        out["result"] = {"job": {"id": "j2", "state": "queued"}}
        seen = 99
    else:
        out["error"] = {"code": -32601, "message": "Method not found.", "data": {"name": "method_not_found"}}
    print("noise that is not JSON", flush=True) if m == "work.start" else None
    print(json.dumps(out), flush=True)
"#;

    #[test]
    fn greets_calls_and_waits_for_jobs() {
        let mut c = Client::spawn(fake(SERVER)).unwrap();
        assert_eq!(c.version, "9.9");
        let mut snapshots = 0;
        let result = c.job("work.start", json!({}), |_| snapshots += 1).unwrap();
        assert_eq!(result["done"], true);
        assert_eq!(snapshots, 1, "one running snapshot before it ended");
        let err = c.call("nothing", json!({})).unwrap_err();
        let rpc = err.downcast_ref::<RpcError>().unwrap();
        assert_eq!((rpc.code, rpc.name.as_str()), (-32601, "method_not_found"));
        assert!(c.alive());
    }

    #[test]
    fn refuses_another_protocol() {
        let other = SERVER.replace(
            r#""minimum_version": 1, "maximum_version": 1"#,
            r#""minimum_version": 2, "maximum_version": 3"#,
        );
        let err = Client::spawn(fake(&other)).err().unwrap();
        assert!(err.to_string().contains("protocol 2–3"), "{err}");
    }

    #[test]
    fn a_service_that_ends_is_an_error_not_a_hang() {
        let err = Client::spawn(fake("import sys; sys.stdin.readline()"))
            .err()
            .unwrap();
        assert!(err.to_string().contains("stopped"), "{err}");
    }
}
