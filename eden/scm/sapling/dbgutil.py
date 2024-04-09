# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
integration with a native debugger like lldb

Check https://lldb.llvm.org/python_api.html for APIs.
"""

import struct
import subprocess
import sys

from . import error
from .i18n import _


class Debugger:
    def __init__(self, pid: int):
        """attach to the given pid for debugging"""
        lldb = import_lldb()
        self.debugger = lldb.SBDebugger.Create()
        self.target = self.debugger.CreateTarget("")
        self.process = self.target.AttachToProcessWithID(
            lldb.SBListener(), pid, lldb.SBError()
        )

    def backtrace_all(self, ui):
        """write backtraces of all threads"""
        lldb = import_lldb()
        target, process = self.target, self.process
        if struct.calcsize("P") != 8:
            raise error.Abort(_("non-64-bit architecture is not yet supported"))

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
            return value.GetSummary().strip('"')

        write = ui.write
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


_lldb_module = None


def import_lldb():
    global _lldb_module

    if _lldb_module is None:
        try:
            lldb_python_path = subprocess.check_output(["lldb", "-P"]).decode().strip()
        except Exception:
            raise error.Abort(_('"lldb -P" is not available'))

        sys.path.append(lldb_python_path)

        import lldb

        _lldb_module = lldb

    return _lldb_module
