#chg-compatible

  $ configure modern
  $ newserver repo
  $ clone repo localrepo
  $ switchrepo localrepo

  $ hg pull --config ui.ssh="hg debugsh -c \"import time; time.sleep(30)\"" --config ui.sshsetuptimeout=1
  pulling from ssh://user@dummy/repo
  timed out establishing the ssh connection, killing ssh
  abort: no suitable response from remote hg!
  [255]
