#!/usr/bin/env python3
"""
STC Autonomous Supervisor Harness.
Orchestrates OpenCode v2 (opencode-go/muse-spark-1.3-contributor) across iterations
to implement roadmap issues, enforce strict DoD verification gates, and record evidence.
"""

import os
import sys
import json
import subprocess
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MODEL_NAME = "opencode-go/muse-spark-1.3-contributor"

def load_env():
    env_file = REPO_ROOT / ".env"
    if env_file.exists():
        with open(env_file, "r") as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, v = line.split("=", 1)
                    os.environ.setdefault(k.strip(), v.strip())

def run_cmd(cmd, cwd=None, timeout=300):
    if cwd is None:
        cwd = REPO_ROOT
    try:
        p = subprocess.run(
            cmd,
            cwd=str(cwd),
            shell=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
        )
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -1, "", f"Command timed out after {timeout}s: {cmd}"
    except Exception as e:
        return -1, "", str(e)

def run_gates():
    """
    DoD Gate Chain:
    1. cargo fmt --check (in studio-core)
    2. cargo clippy --all-targets -- -D warnings (in studio-core)
    3. cargo test (in studio-core)
    4. prek run --all-files
    5. npm run build (in cockpit)
    """
    results = {}
    
    # 1. Cargo fmt
    code, out, err = run_cmd("cargo fmt --check", cwd=REPO_ROOT / "studio-core")
    results["fmt"] = (code == 0, out + err)
    if code != 0:
        return False, "cargo fmt failed", results

    # 2. Cargo clippy
    code, out, err = run_cmd("cargo clippy --all-targets -- -D warnings", cwd=REPO_ROOT / "studio-core")
    results["clippy"] = (code == 0, out + err)
    if code != 0:
        return False, "cargo clippy failed", results

    # 3. Cargo test
    code, out, err = run_cmd("cargo test", cwd=REPO_ROOT / "studio-core")
    results["test"] = (code == 0, out + err)
    if code != 0:
        return False, "cargo test failed", results

    # 4. Prek
    code, out, err = run_cmd("prek run --all-files", cwd=REPO_ROOT)
    results["prek"] = (code == 0, out + err)
    # If prek not found or exits, check code
    if code != 0:
        return False, "prek gates failed", results

    # 5. Cockpit build
    code, out, err = run_cmd("npm run build", cwd=REPO_ROOT / "cockpit")
    results["cockpit"] = (code == 0, out + err)
    if code != 0:
        return False, "cockpit npm build failed", results

    return True, "All gates green", results

def dispatch_opencode(prompt, cwd=None, timeout=600):
    if cwd is None:
        cwd = REPO_ROOT
    cmd = f'opencode run --auto -m {MODEL_NAME} "{prompt}"'
    print(f"[Supervisor] Dispatching to OpenCode ({MODEL_NAME})...")
    return run_cmd(cmd, cwd=cwd, timeout=timeout)

def get_open_issues():
    code, out, err = run_cmd("gh issue list --limit 50 --json number,title,labels")
    if code == 0:
        return json.loads(out)
    return []

def get_issue_body(issue_num):
    code, out, err = run_cmd(f"gh issue view {issue_num}")
    if code == 0:
        return out
    return ""

def comment_and_close_issue(issue_num, comment):
    run_cmd(f'gh issue comment {issue_num} --body "{comment}"')
    run_cmd(f'gh issue close {issue_num} --comment "Closed with evidence by autonomous supervisor."')

if __name__ == "__main__":
    load_env()
    print("[Supervisor] Environment loaded. Testing verification gates...")
    ok, msg, res = run_gates()
    print(f"[Supervisor] Initial gate status: ok={ok}, msg={msg}")
    if not ok:
        print("[Supervisor] Warning: Initial gates failed:", msg)
        for k, v in res.items():
            print(f"  {k}: passed={v[0]}")
    else:
        print("[Supervisor] Baseline is pristine! Ready for iterations.")
