# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
integration with a native debugger like lldb

Check https://lldb.llvm.org/python_api.html for APIs.

This file might run as an independent lldb script when lldb cannot be imported
(because Python version mismatch). Try to avoid depending on other modules.
"""

import struct
import subprocess
import sys


def backtrace_all(ui, pid: int):
    """write backtraces of all threads of the given pid.
    Runs inside Sapling Python environment.
    """
    import inspect
    import os
    import tempfile

    import bindings

    python_source = inspect.getsource(sys.modules["sapling.dbgutil"])

    with tempfile.TemporaryDirectory(prefix="saplinglldb") as dir:
        python_script_path = os.path.join(dir, "dbgutil.py")
        with open(python_script_path, "wb") as f:
            f.write(python_source.encode())
        args = [
            ui.config("ui", "lldb") or "lldb",
            "-b",
            "--source-quietly",
            "-o",
            f"command script import {python_script_path}",
            "-o",
            f"bta {pid}",
        ]
        subprocess.run(args)


def _lldb_backtrace_all(pid: int):
    """intended to be executed as a lldb script.
    Runs inside lldb Python environment, outside Sapling environment.
    """
    import lldb

    write = sys.stdout.write
    debugger = lldb.debugger
    target = debugger.CreateTarget("")
    process = target.AttachToProcessWithID(lldb.SBListener(), pid, lldb.SBError())
    if struct.calcsize("P") != 8:
        write("non-64-bit architecture is not yet supported")
        return

    def read_u64(address: int) -> int:
        """read u64 from an address"""
        return struct.unpack("Q", process.ReadMemory(address, 8, lldb.SBError()))[0]

    def resolve_frame(frame) -> str:
        """extract python stack info from a frame.
        The frame should be a Sapling_PyEvalFrame function call.
        """
        # Sapling_PyEvalFrame(PyThreadState* tstate, PyFrameObject* f, int exc)
        # push   %rbp
        # mov    %rsp,%rbp        ; FP
        # sub    $0x20,%rsp       ; SP = FP - 0x20
        # mov    %rdi,-0x18(%rbp)
        # mov    %rsi,-0x10(%rbp) ; PyFrame f, at FP - 0x10
        # mov    %edx,-0x4(%rbp)
        fp: int = frame.fp
        sp: int = frame.sp
        if fp - sp == 0x20:
            try:
                python_frame_address = read_u64(fp - 0x10)
                return resolve_python_frame(python_frame_address)
            except Exception as e:
                return f"<error {e} {fp - 0x10}>"
        return ""

    def resolve_python_frame(python_frame_address: int) -> str:
        # NOTE: `sapling_cext_evalframe_resolve_frame` needs the Python GIL
        # to be "safe". However, it is likely just reading a bunch of
        # objects (ex. frame, code, str) and those objects are not being
        # GC-ed (since the call stack need them). So it is probably okay.
        value = target.EvaluateExpression(
            f"(char *)(sapling_cext_evalframe_resolve_frame((size_t){python_frame_address}))"
        )
        return (value.GetSummary() or "").strip('"')

    for thread in process.threads:
        write(("%r\n") % thread)
        for i, frame in enumerate(thread.frames):
            name = frame.GetDisplayFunctionName()
            if name == "Sapling_PyEvalFrame":
                resolved = resolve_frame(frame)
                if resolved:
                    # The "frame #i" format matches the repr(frame) style.
                    write(f"  frame #{i}: {resolved}\n")
                    continue
            if name:
                write(f"  {repr(frame)}\n")
        write("\n")


def _lldb_backtrace_all_command(debugger, command, result, internal_dict):
    # lldb command to take a `pid` and call _lldb_backtrace_all.
    pid = int(command.split()[-1])
    _lldb_backtrace_all(pid)


def __lldb_init_module(debugger, internal_dict):
    # When imported by lldb 'command script import', this function is called.
    # Add a 'bta' command to call _lldb_backtrace_all.
    debugger.HandleCommand(
        f"command script add -f {__name__}._lldb_backtrace_all_command bta"
    )
