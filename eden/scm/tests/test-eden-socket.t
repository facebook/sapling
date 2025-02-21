#require eden

setup backing repo

  $ newclientrepo backingrepo
  $ eden clone --allow-empty-repo $TESTTMP/backingrepo $TESTTMP/wcrepo
  Cloning new repository at $TESTTMP/wcrepo...
  Success.  Checked out commit 00000000

Print the path of an existing socket file
  $ EDENFSCTL_ONLY_RUST=true eden socket
  .* (re)
  >>> socket_cmd_output = _.strip()
  >>> expected_tail = "socket (no-eol)"
  >>> assert socket_cmd_output.endswith(expected_tail)
  >>> assert not socket_cmd_output.startswith("Error finding socket file")
  >>> import os
  >>> assert os.path.exists(socket_cmd_output[:-9])
