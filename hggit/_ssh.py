class SSHVendor(object):
    """Parent class for ui-linked Vendor classes."""


def generate_ssh_vendor(ui):
    """
    Allows dulwich to use hg's ui.ssh config. The dulwich.client.get_ssh_vendor
    property should point to the return value.
    """

    class _Vendor(SSHVendor):
        def connect_ssh(self, host, command, username=None, port=None):
            from dulwich.client import SubprocessWrapper
            from mercurial import util
            import subprocess

            sshcmd = ui.config("ui", "ssh", "ssh")
            args = util.sshargs(sshcmd, host, username, port)

            proc = subprocess.Popen([sshcmd, args] + command,
                                    stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE)
            return SubprocessWrapper(proc)

    return _Vendor
