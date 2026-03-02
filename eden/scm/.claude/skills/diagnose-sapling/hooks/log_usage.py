# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict

"""
PostToolUse hook for logging diagnose-sapling skill telemetry to Scuba.

Scribe category: perfpipe_diagnose_sapling_skill
Scuba table: diagnose_sapling_skill

Schema groups:
  Core identifiers: time, session_id, invocation_id, unixname, hostname, os_type
  Problem classification: problem_domain, symptom_category, error_message, sl_command
  Diagnostic steps: commands_run, sl_doctor_run, eden_doctor_run, eden_healthy,
                    rage_generated, diagnosis_summary, findings
  Outcome: outcome, duration_ms
  Resolution details: resolution_type, resolution_detail, root_cause
  Non-resolution details: unresolved_reason, escalation_target,
                          escalation_post_created

Hook data schema (from Claude Code):
  Common: session_id, hook_event_name, timestamp, transcript_path, cwd
  PostToolUse: tool_name, tool_input, tool_use_id, tool_result
"""

import json
import os
import platform
import socket
import subprocess
import sys
import time
from typing import Any, Dict, Optional


SCUBA_DATASET: str = "diagnose_sapling_skill"
SCRIBE_CATEGORY: str = f"perfpipe_{SCUBA_DATASET}"


def log_to_scuba(
    session_id: str,
    tool_use_id: str,
    success: bool,
) -> None:
    """Log skill invocation to Scuba via scribe_cat."""
    try:
        sample: Dict[str, Any] = {
            "int": {
                "time": int(time.time()),
                "success": 1 if success else 0,
            },
            "normal": {
                "session_id": session_id,
                "tool_use_id": tool_use_id,
                "unixname": os.environ.get("USER", "unknown"),
                "hostname": _get_hostname(),
                "os_type": platform.system().lower(),
                "invocation_id": os.environ.get("META_CLAUDE_INVOCATION_ID", ""),
            },
        }

        sample_json: str = json.dumps(sample)

        subprocess.run(
            ["scribe_cat", SCRIBE_CATEGORY],
            input=sample_json.encode(),
            capture_output=True,
            timeout=5,
        )
    except Exception:
        # Fail silently — never break Claude's workflow
        pass


def _get_hostname() -> str:
    """Get hostname, works on all platforms."""
    try:
        return socket.gethostname()
    except Exception:
        return "unknown"


def handle_post_tool_use(hook_data: Dict[str, Any]) -> None:
    """Handle PostToolUse hook event for diagnose-sapling skill."""
    tool_name: str = hook_data.get("tool_name", "")
    if tool_name != "Skill":
        return

    tool_input: Dict[str, Any] = hook_data.get("tool_input", {})
    skill_name: Optional[str] = tool_input.get("skill", "")
    if skill_name != "diagnose-sapling":
        return

    session_id: str = hook_data.get("session_id", "unknown")
    tool_use_id: str = hook_data.get("tool_use_id", "")
    tool_result: Any = hook_data.get("tool_result", {})
    success: bool = not isinstance(tool_result, dict) or not tool_result.get("error")

    log_to_scuba(
        session_id=session_id,
        tool_use_id=tool_use_id,
        success=success,
    )


def main() -> None:
    """
    Main entry point for the logging hook.

    Reads hook data from stdin and logs skill usage to Scuba.
    """
    try:
        if sys.stdin.isatty():
            return

        stdin_data: str = sys.stdin.read()
        if not stdin_data:
            return

        hook_data: Dict[str, Any] = json.loads(stdin_data)

        if hook_data.get("hook_event_name") != "PostToolUse":
            return

        handle_post_tool_use(hook_data)
    except Exception:
        # Fail silently — never break Claude's workflow
        pass


if __name__ == "__main__":
    main()
