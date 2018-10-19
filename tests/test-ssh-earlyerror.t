  $ hg --config ui.ssh="echo ssh: SSH is not working 1>&2; exit 1;" clone ssh://foo//bar
  ssh: SSH is not working
  abort: no suitable response from remote hg!
  [255]
