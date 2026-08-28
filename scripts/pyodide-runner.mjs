import { loadPyodide } from "pyodide";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);

try {
  const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (typeof request.code !== "string" || request.code.length > 64_000) {
    throw new Error("invalid Python source");
  }

  const pyodide = await loadPyodide();
  pyodide.globals.set("__svetsec_code", request.code);
  const output = await pyodide.runPythonAsync(`
import ast
import builtins
import contextlib
import io
import traceback

_tree = ast.parse(__svetsec_code, filename="<article>", mode="exec")
_blocked_modules = {"js", "pyodide", "_pyodide"}
_blocked_calls = {"__import__", "breakpoint", "compile", "eval", "exec", "input", "open"}
for _node in ast.walk(_tree):
    if isinstance(_node, (ast.Import, ast.ImportFrom)):
        _names = [alias.name.split(".")[0] for alias in _node.names] if isinstance(_node, ast.Import) else [(_node.module or "").split(".")[0]]
        if any(name in _blocked_modules for name in _names):
            raise PermissionError("browser and host bridges are disabled")
    if isinstance(_node, ast.Call) and isinstance(_node.func, ast.Name) and _node.func.id in _blocked_calls:
        raise PermissionError(f"{_node.func.id}() is disabled")
    if isinstance(_node, ast.Call) and isinstance(_node.func, ast.Attribute) and _node.func.attr == "import_module":
        raise PermissionError("dynamic imports are disabled")

_stdout = io.StringIO()
_stderr = io.StringIO()
_original_import = builtins.__import__
def _safe_import(name, globals=None, locals=None, fromlist=(), level=0):
    if name.split(".")[0] in _blocked_modules:
        raise PermissionError("browser and host bridges are disabled")
    return _original_import(name, globals, locals, fromlist, level)

_safe_builtins = vars(builtins).copy()
_safe_builtins["__import__"] = _safe_import
for _name in _blocked_calls - {"__import__"}:
    _safe_builtins.pop(_name, None)
_globals = {"__name__": "__main__", "__builtins__": _safe_builtins}
try:
    with contextlib.redirect_stdout(_stdout), contextlib.redirect_stderr(_stderr):
        exec(compile(_tree, "<article>", "exec"), _globals)
except BaseException:
    traceback.print_exc(file=_stderr)

(_stdout.getvalue() + _stderr.getvalue())[:16384]
  `);
  console.log(JSON.stringify({ output: String(output) }));
} catch (error) {
  console.log(JSON.stringify({ error: String(error?.stack || error) }));
  process.exitCode = 1;
}
