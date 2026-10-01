#!/usr/bin/env python3
"""Exercise the actual pre-load guards without importing or loading a model."""
import ast
import types
from pathlib import Path

root = Path(__file__).parent
for name, variable in (("serve.py", "DEVICE"), ("e1.py", "device")):
    tree = ast.parse((root / name).read_text())
    statements = tree.body if name == "serve.py" else next(n.body for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "main")
    index = next(i for i, n in enumerate(statements) if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == variable for t in n.targets))
    model_index = next(i for i, n in enumerate(statements) if any(
        isinstance(call, ast.Call) and isinstance(call.func, ast.Name) and call.func.id == "SentenceTransformer"
        for call in ast.walk(n)))
    assert index + 1 < model_index, "GPU guard must run before model loading"
    code = compile(ast.Module(body=statements[index:index + 2], type_ignores=[]), name, "exec")
    for device, available, refused in (("cpu", True, True), ("cuda", False, True), ("cuda", True, False)):
        scope = {"os": types.SimpleNamespace(environ={"USER2_DEVICE": device}),
                 "torch": types.SimpleNamespace(cuda=types.SimpleNamespace(is_available=lambda: available))}
        try:
            exec(code, scope)
        except SystemExit as error:
            assert refused and "GPU-only" in str(error)
        else:
            assert not refused
print("CPU and missing-CUDA inference refused before model loading; CUDA accepted")
