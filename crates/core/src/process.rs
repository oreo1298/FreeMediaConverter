// SPDX-License-Identifier: GPL-3.0-or-later
//! Running external programs without a shell, with timeouts, behind a trait
//! so that discovery and probing can be tested against recorded fixtures.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// A program invocation: argv only, never a shell string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub timeout: Option<Duration>,
    pub stdin: Option<Vec<u8>>,
}

impl Invocation {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Invocation {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            timeout: Some(Duration::from_secs(30)),
            stdin: None,
        }
    }
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Self {
        self.args.push(a.as_ref().to_os_string());
        self
    }
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }
    pub fn env(mut self, k: impl AsRef<OsStr>, v: impl AsRef<OsStr>) -> Self {
        self.env
            .push((k.as_ref().to_os_string(), v.as_ref().to_os_string()));
        self
    }
    pub fn timeout(mut self, t: Option<Duration>) -> Self {
        self.timeout = t;
        self
    }
    /// Program basename, for fixture lookup and messages.
    pub fn program_name(&self) -> String {
        self.program
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.program.to_string_lossy().into_owned())
    }
    /// Arguments as lossy strings (for matching / display).
    pub fn args_lossy(&self) -> Vec<String> {
        self.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
    /// True if `flag` is immediately followed by `value` in the argv.
    pub fn has_pair(&self, flag: &str, value: &str) -> bool {
        self.args
            .windows(2)
            .any(|w| w[0] == OsStr::new(flag) && w[1] == OsStr::new(value))
    }
    /// The value following `flag`, if any.
    pub fn value_of(&self, flag: &str) -> Option<String> {
        self.args
            .windows(2)
            .find(|w| w[0] == OsStr::new(flag))
            .map(|w| w[1].to_string_lossy().into_owned())
    }
}

/// Captured result of a finished program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmdOutput {
    /// Exit code (None if killed by a signal or timed out).
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

impl CmdOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        CmdOutput {
            code: Some(0),
            stdout: stdout.into(),
            ..Default::default()
        }
    }
    pub fn fail(code: i32, stderr: impl Into<Vec<u8>>) -> Self {
        CmdOutput {
            code: Some(code),
            stderr: stderr.into(),
            ..Default::default()
        }
    }
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
    /// Turn a failure into an [`Error`].
    pub fn into_result(self, program: &str) -> Result<CmdOutput> {
        if self.timed_out {
            return Err(Error::Timeout {
                program: program.to_string(),
                seconds: 0.0,
            });
        }
        if self.code != Some(0) {
            return Err(Error::Failed {
                program: program.to_string(),
                code: self.code,
                stderr_tail: crate::error::tail_lines(&self.stderr, 6),
            });
        }
        Ok(self)
    }
}

/// Something that can run an [`Invocation`]. Implemented by [`SystemRunner`]
/// (real processes) and [`FixtureRunner`] (recorded answers).
pub trait Runner: Send + Sync {
    fn run(&self, inv: &Invocation) -> Result<CmdOutput>;
    /// Whether a program exists (used for optional tools like nvidia-smi).
    fn exists(&self, program: &str) -> bool;
}

pub type SharedRunner = Arc<dyn Runner>;

/// Environment applied to every child: parseable output and PCI-ordered CUDA
/// device indices (so `-gpu N` matches nvidia-smi).
pub fn base_env(cmd: &mut Command) {
    cmd.env("LC_ALL", "C");
    cmd.env("CUDA_DEVICE_ORDER", "PCI_BUS_ID");
    // Never let ffmpeg/ffprobe read a user's AV_LOG_FORCE_COLOR into our parsers.
    cmd.env_remove("AV_LOG_FORCE_COLOR");
    cmd.env("AV_LOG_FORCE_NOCOLOR", "1");
}

/// Runs real processes.
#[derive(Debug, Default, Clone)]
pub struct SystemRunner;

impl SystemRunner {
    pub fn shared() -> SharedRunner {
        Arc::new(SystemRunner)
    }
}

/// Search `$PATH` for an executable named `name` (no shell involved).
pub fn which(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

pub fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl Runner for SystemRunner {
    fn exists(&self, program: &str) -> bool {
        which(program).is_some()
    }

    fn run(&self, inv: &Invocation) -> Result<CmdOutput> {
        let mut cmd = Command::new(&inv.program);
        cmd.args(&inv.args);
        base_env(&mut cmd);
        for (k, v) in &inv.env {
            cmd.env(k, v);
        }
        cmd.stdin(if inv.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        // Put the child in its own process group so a timeout kill also
        // reaches helpers it may have spawned.
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|source| Error::Spawn {
            program: inv.program_name(),
            source,
        })?;

        if let Some(data) = &inv.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                let data = data.clone();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(&data);
                });
            }
        }
        let mut out = child.stdout.take().expect("piped stdout");
        let mut err = child.stderr.take().expect("piped stderr");
        let t_out = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            buf
        });
        let t_err = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            buf
        });

        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(_) => break None,
            }
            if let Some(limit) = inv.timeout {
                if start.elapsed() >= limit {
                    timed_out = true;
                    // SAFETY: plain kill(2) on the child's process group.
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let stdout = t_out.join().unwrap_or_default();
        let stderr = t_err.join().unwrap_or_default();
        Ok(CmdOutput {
            code: status.and_then(|s| s.code()),
            stdout,
            stderr,
            timed_out,
        })
    }
}

type Rule = Box<dyn Fn(&Invocation) -> Option<CmdOutput> + Send + Sync>;

/// Answers invocations from rules (first match wins). Used by tests and by
/// simulated hardware profiles.
#[derive(Default)]
pub struct FixtureRunner {
    rules: Vec<Rule>,
    programs: Vec<String>,
    log: std::sync::Mutex<Vec<Invocation>>,
}

impl FixtureRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare that `program` exists (for [`Runner::exists`]).
    pub fn with_program(mut self, program: &str) -> Self {
        self.programs.push(program.to_string());
        self
    }

    /// Add a matching rule.
    pub fn rule(
        mut self,
        f: impl Fn(&Invocation) -> Option<CmdOutput> + Send + Sync + 'static,
    ) -> Self {
        self.rules.push(Box::new(f));
        self
    }

    /// Answer `program args...` (exact args) with `stdout`.
    pub fn exact(self, program: &str, args: &[&str], stdout: impl Into<Vec<u8>>) -> Self {
        let program = program.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let stdout = stdout.into();
        self.with_program(&program.clone()).rule(move |inv| {
            (inv.program_name() == program && inv.args_lossy() == args)
                .then(|| CmdOutput::ok(stdout.clone()))
        })
    }

    /// Every invocation seen so far.
    pub fn calls(&self) -> Vec<Invocation> {
        self.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Runner for FixtureRunner {
    fn exists(&self, program: &str) -> bool {
        let base = Path::new(program)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.programs.iter().any(|p| p == program || *p == base)
    }

    fn run(&self, inv: &Invocation) -> Result<CmdOutput> {
        if let Ok(mut log) = self.log.lock() {
            log.push(inv.clone());
        }
        for rule in &self.rules {
            if let Some(out) = rule(inv) {
                return Ok(out);
            }
        }
        Err(Error::Spawn {
            program: inv.program_name(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no fixture for {:?} {:?}", inv.program, inv.args_lossy()),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_runner_captures_output_and_code() {
        let r = SystemRunner;
        let out = r
            .run(&Invocation::new("/bin/echo").arg("-n").arg("hello world"))
            .unwrap();
        assert!(out.success());
        assert_eq!(out.stdout_str(), "hello world");

        let out = r.run(&Invocation::new("/bin/false")).unwrap();
        assert_eq!(out.code, Some(1));
    }

    #[test]
    fn system_runner_passes_args_verbatim() {
        // A shell would expand these; argv must not.
        let weird = "$(touch /tmp/should-not-exist) ; `id` | && *";
        let out = SystemRunner
            .run(&Invocation::new("/bin/echo").arg(weird))
            .unwrap();
        assert_eq!(out.stdout_str().trim_end(), weird);
    }

    #[test]
    fn system_runner_times_out() {
        let out = SystemRunner
            .run(
                &Invocation::new("/bin/sleep")
                    .arg("5")
                    .timeout(Some(Duration::from_millis(200))),
            )
            .unwrap();
        assert!(out.timed_out);
        assert!(!out.success());
    }

    #[test]
    fn system_runner_feeds_stdin() {
        let mut inv = Invocation::new("/bin/cat");
        inv.stdin = Some(b"piped".to_vec());
        let out = SystemRunner.run(&inv).unwrap();
        assert_eq!(out.stdout_str(), "piped");
    }

    #[test]
    fn fixture_runner_matches_rules() {
        let r = FixtureRunner::new().exact("ffmpeg", &["-version"], "ffmpeg version 9.0.2");
        let out = r
            .run(&Invocation::new("/usr/bin/ffmpeg").arg("-version"))
            .unwrap();
        assert_eq!(out.stdout_str(), "ffmpeg version 9.0.2");
        assert!(r.exists("ffmpeg"));
        assert!(r.run(&Invocation::new("ffmpeg").arg("-x")).is_err());
        assert_eq!(r.calls().len(), 2);
    }

    #[test]
    fn invocation_helpers() {
        let inv = Invocation::new("ffmpeg").args(["-c:v", "h264_nvenc", "-f", "null"]);
        assert!(inv.has_pair("-c:v", "h264_nvenc"));
        assert_eq!(inv.value_of("-f").as_deref(), Some("null"));
        assert_eq!(inv.value_of("-x"), None);
    }
}
