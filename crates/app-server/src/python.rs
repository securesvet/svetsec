use std::{env, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, process::Command, sync::Semaphore, time::timeout};

const MAX_CODE_BYTES: usize = 64_000;
const MAX_OUTPUT_BYTES: usize = 16_384;
const RUN_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone)]
pub struct PyodideRunner {
    node: Arc<str>,
    script: Arc<PathBuf>,
    node_modules: Arc<PathBuf>,
    permits: Arc<Semaphore>,
}

#[derive(Serialize)]
struct RunRequest<'a> {
    code: &'a str,
}

#[derive(Deserialize)]
struct RunResponse {
    output: Option<String>,
    error: Option<String>,
}

impl PyodideRunner {
    pub fn from_env() -> Self {
        Self::new(
            env::var("SVETSEC_PYODIDE_NODE").unwrap_or_else(|_| "node".into()),
            env::var("SVETSEC_PYODIDE_RUNNER")
                .unwrap_or_else(|_| "scripts/pyodide-runner.mjs".into()),
            env::var("SVETSEC_PYODIDE_NODE_MODULES").unwrap_or_else(|_| "node_modules".into()),
        )
    }

    fn new(
        node: impl Into<Arc<str>>,
        script: impl Into<PathBuf>,
        node_modules: impl Into<PathBuf>,
    ) -> Self {
        Self {
            node: node.into(),
            script: Arc::new(script.into()),
            node_modules: Arc::new(node_modules.into()),
            permits: Arc::new(Semaphore::new(2)),
        }
    }

    pub async fn run(&self, code: &str) -> Result<String> {
        if code.trim().is_empty() || code.len() > MAX_CODE_BYTES {
            bail!("Python block is empty or too large");
        }
        let _permit = self
            .permits
            .acquire()
            .await
            .context("Python runner is shutting down")?;
        let script = self
            .script
            .canonicalize()
            .context("Pyodide runner script is missing")?;
        self.node_modules
            .canonicalize()
            .context("node_modules is missing; run npm install")?;
        let mut command = Command::new(self.node.as_ref());
        command
            .arg("--max-old-space-size=192")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Ok(path) = env::var("PATH") {
            command.env_clear().env("PATH", path);
        } else {
            command.env_clear();
        }
        let mut child = command
            .spawn()
            .context("could not start Node.js for Pyodide")?;
        let request = serde_json::to_vec(&RunRequest { code })?;
        child
            .stdin
            .take()
            .context("Python runner stdin unavailable")?
            .write_all(&request)
            .await?;
        let output = timeout(RUN_TIMEOUT, child.wait_with_output())
            .await
            .context("Python execution timed out")??;
        if output.stdout.len() > MAX_OUTPUT_BYTES * 2 {
            bail!("Python runner returned too much data");
        }
        let response = serde_json::from_slice::<RunResponse>(&output.stdout)
            .context("Pyodide runner returned invalid JSON")?;
        if !output.status.success() {
            bail!(
                "{}",
                response
                    .error
                    .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).into_owned())
            );
        }
        Ok(response.output.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use svetsec_core::python_code_blocks;

    use super::PyodideRunner;

    #[test]
    fn only_python_fences_are_selected_for_execution() {
        assert_eq!(
            python_code_blocks("```rust\nfn main() {}\n```\n```python\nprint(2 + 2)\n```"),
            vec!["print(2 + 2)"]
        );
    }

    #[tokio::test]
    async fn installed_pyodide_executes_cryptography_example() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        if !workspace.join("node_modules/pyodide").is_dir() {
            return;
        }
        let output = PyodideRunner::new(
            "node",
            workspace.join("scripts/pyodide-runner.mjs"),
            workspace.join("node_modules"),
        )
        .run("import hashlib; print(hashlib.sha256(b'svetsec').hexdigest())")
        .await
        .unwrap();
        assert_eq!(
            output.trim(),
            "3d2b3746bacde7ba190ffc926ef74e019e37941b548ada2c4f3b9f71bdbdf2f2"
        );
    }
}
