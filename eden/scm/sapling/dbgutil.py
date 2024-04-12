# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
integration with a native debugger like lldb

Check https://lldb.llvm.org/python_api.html for APIs.

This file runs standalone by lldb's Python interperter. It does not have access
to `bindings` or other Sapling modules. Do not import Sapling modules here.

There are 2 ways to use this feature,
- Use `debugbt` command.
- Use `lldb -p <pid>`, then run `command script import ./dbgutil.py`,
  then use the `bta` command.
"""

import functools
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
        if ui.formatted:
            out_path = ""
        else:
            # Buffer output so we
            out_path = os.path.join(dir, "bta_output.txt")
        with open(python_script_path, "wb") as f:
            f.write(python_source.encode())
        args = [
            ui.config("ui", "lldb") or "lldb",
            "-b",
            "--source-quietly",
            "-o",
            f"command script import {python_script_path}",
            "-o",
            f"bta {pid}{out_path and ' ' + out_path}",
        ]
        subprocess.run(args, stdout=(subprocess.DEVNULL if out_path else None))
        if out_path:
            with open(out_path, "rb") as f:
                data = f.read()
                ui.writebytes(data)


def _lldb_backtrace_all_attach_pid(pid, write):
    """Attach to a pid and write its backtraces.
    Runs inside lldb Python environment, outside Sapling environment.
    """
    import lldb

    debugger = lldb.debugger
    target = debugger.CreateTarget("")
    process = target.AttachToProcessWithID(lldb.SBListener(), pid, lldb.SBError())
    try:
        _lldb_backtrace_all_for_process(target, process, write)
    finally:
        if sys.platform == "win32":
            # Attempt to resume the suspended process. "Detach()" alone does not
            # seem to resume it...
            debugger.SetAsync(True)
            process.Continue()
        process.Detach()


def _lldb_backtrace_all_for_process(target, process, write):
    """Write backtraces for the given lldb target/process.
    Runs inside lldb Python environment, outside Sapling environment.
    """
    import lldb

    if target.addr_size != 8:
        write("non-64-bit architecture is not yet supported\n")
        return

    def read_u64(address: int) -> int:
        """read u64 from an address"""
        return struct.unpack("Q", process.ReadMemory(address, 8, lldb.SBError()))[0]

    def resolve_frame(frame) -> str:
        """extract python stack info from a frame.
        The frame should be a Sapling_PyEvalFrame function call.
        """
        # Sapling_PyEvalFrame(PyThreadState* tstate, PyFrameObject* f, int exc)
        #
        # x64:
        #   pushq  %rbp
        #   movq   %rsp, %rbp        ; FP
        #   subq   $0x20, %rsp       ; SP = FP - 0x20
        #   movq   %rdi, -0x8(%rbp)
        #   movq   %rsi, -0x10(%rbp) ; PyFrame f at FP - 0x10, or SP + 0x10
        #   movl   %edx, -0x14(%rbp)
        #   movq   -0x8(%rbp), %rdi
        #   movq   -0x10(%rbp), %rsi
        #   movl   -0x14(%rbp), %edx
        #   callq  0x1034bddee       ; symbol stub for: _PyEval_EvalFrameDefault
        #   addq   $0x20, %rsp
        #   popq   %rbp
        #   retq
        #
        # arm64 (x29 is FP):
        #   sub    sp, sp, #0x30
        #   stp    x29, x30, [sp, #0x20]
        #   add    x29, sp, #0x20      ; FP = SP + 0x20
        #   stur   x0, [x29, #-0x8]    ; x0 is 1st arg (tstate)
        #   str    x1, [sp, #0x10]     ; x1 is 2nd arg, `f`, at SP + 0x10
        #   str    w2, [sp, #0xc]
        #   ldur   x0, [x29, #-0x8]
        #   ldr    x1, [sp, #0x10]
        #   ldr    w2, [sp, #0xc]
        #   bl     0x1046b6140          ; symbol stub for: _PyEval_EvalFrameDefault
        #   ldp    x29, x30, [sp, #0x20]
        #   add    sp, sp, #0x30
        #   ret
        #
        # x64 MSVC:
        #   ; Sapling_PyEvalFrame(PyThreadState* tstate, PyFrame* f, int exc) {
        #   push        rbp
        #   sub         rsp,40h
        #   lea         rbp,[rsp+40h]
        #   mov         dword ptr [rbp-4],r8d
        #   mov         qword ptr [rbp-18h],rdx       ; FP - 0x18
        #   mov         qword ptr [rbp-10h],rcx
        #   ; return _PyEval_EvalFrameDefault(tstate, f, exc);
        #   mov         r8d,dword ptr [exc]
        #   mov         rdx,qword ptr [f]
        #   mov         rcx,qword ptr [tstate]
        #   call        qword ptr [__imp__PyEval_EvalFrameDefault (07FF748CDDF40h)]
        #   nop
        #   add         rsp,40h
        #   pop         rbp
        #   ret
        fp: int = frame.fp
        sp: int = frame.sp
        ptr_addr = None
        if fp - sp == 0x40 and sys.platform == "win32":
            ptr_addr = fp - 0x18
        elif fp - sp == 0x20:
            ptr_addr = fp - 0x10
        if ptr_addr is not None:
            try:
                python_frame_address = read_u64(ptr_addr)
                return resolve_python_frame(python_frame_address)
            except Exception as e:
                return f"<error {e} {ptr_addr}>"
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
        frames = []  # [(frame | None, line)]
        has_resolved_python_frame = False
        for i, frame in enumerate(thread.frames):
            name = frame.GetDisplayFunctionName()
            if name == "Sapling_PyEvalFrame":
                resolved = resolve_frame(frame)
                if resolved:
                    has_resolved_python_frame = True
                    # The "frame #i" format matches the repr(frame) style.
                    frames.append((None, f"  frame #{i}: {resolved}\n"))
                    continue
            if name:
                frames.append((frame, f"  {repr(frame)}\n"))
        if has_resolved_python_frame:
            # If any Python frame is resolved, strip out noisy frames like _PyEval_EvalFrameDefault.
            frames = [
                (frame, line)
                for frame, line in frames
                if not _is_cpython_function(frame)
            ]
        write("".join(line for _frame, line in frames))
        write("\n")


def _is_cpython_function(frame) -> bool:
    return frame is not None and "python" in (frame.module.file.basename or "").lower()


def _lldb_backtrace_all_command(debugger, command, exe_ctx, result, internal_dict):
    """lldb command: bta [pid] [PATH]. Write Python+Rust traceback to stdout or PATH."""
    args = command.split(" ", 1)
    if len(args) >= 1 and args[0]:
        pid = int(args[0])
        impl = functools.partial(_lldb_backtrace_all_attach_pid, pid)
    else:
        target = exe_ctx.target
        process = exe_ctx.process
        impl = functools.partial(_lldb_backtrace_all_for_process, target, process)

    path = args[1].strip() if len(args) >= 2 else None
    if path:
        with open(path, "w", newline="\n") as f:
            impl(f.write)
            f.flush()
    else:
        impl(sys.stdout.write)


def __lldb_init_module(debugger, internal_dict):
    # When imported by lldb 'command script import', this function is called.
    # Add a 'bta' command to call _lldb_backtrace_all.
    debugger.HandleCommand(
        f"command script add -f {__name__}._lldb_backtrace_all_command bta"
    )
