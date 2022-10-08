import ghstack.eden_shell
import ghstack.shell


def is_eden_working_copy(sh: ghstack.shell.Shell) -> bool:
    return isinstance(sh, ghstack.eden_shell.EdenShell)
