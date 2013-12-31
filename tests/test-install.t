hg debuginstall
  $ hg debuginstall
  checking encoding (ascii)...
  showing Python executable (*) (glob)
  showing Python version (2.*) (glob)
  checking Python lib (*lib*)... (glob)
  checking installed modules (*mercurial)... (glob)
  checking templates (*mercurial?templates)... (glob)
  checking commit editor...
  checking username...
  no problems detected

hg debuginstall with no username
  $ HGUSER= hg debuginstall
  checking encoding (ascii)...
  showing Python executable (*) (glob)
  showing Python version (2.*) (glob)
  checking Python lib (*lib*)... (glob)
  checking installed modules (*mercurial)... (glob)
  checking templates (*mercurial?templates)... (glob)
  checking commit editor...
  checking username...
   no username supplied
   (specify a username in your configuration file)
  1 problems detected, please check your install!
  [1]
