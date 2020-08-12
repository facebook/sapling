#chg-compatible

  $ configure modern
  $ newserver repo
  $ clone repo localrepo
  $ switchrepo localrepo

  $ cat >> sleep30.py <<EOF
  > import time
  > time.sleep(30)
  > EOF
  $ hg pull --config ui.ssh="$PYTHON ./sleep30.py" --config ui.sshsetuptimeout=1
  pulling from ssh://user@dummy/repo
  timed out establishing the ssh connection, killing ssh
  abort: no suitable response from remote hg!
  [255]
