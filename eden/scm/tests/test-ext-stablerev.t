#chg-compatible
  $ configure modernclient
  $ newclientrepo
  $ enable stablerev
  $ hg debugdrawdag <<'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ setconfig alias.log="log -T '[{node|short}]: {desc}\n'"

# Basics

If the script doesn't return anything, an abort is raised:
  $ printf "#!/bin/bash\n" > stable.sh
  $ chmod +x stable.sh
  $ setconfig stablerev.script=stable.sh
  $ hg log -r "getstablerev()" --debug
  Executing script: $TESTTMP/repo1/stable.sh
  setting current working directory to: $TESTTMP/repo1
  script stdout:
  
  abort: stable rev returned by script (stable.sh) was empty
  [255]

Make the script return something:
  $ printf "#!/bin/bash\n\necho 'B'" > stable.sh
  $ hg log -r "getstablerev()"
  [112478962961]: B

Change the script, change the result:
  $ printf "#!/bin/bash\n\necho 'C'" > stable.sh
  $ hg log -r "getstablerev()"
  [26805aba1e60]: C

The script is always run relative to repo root:
  $ mkdir subdir
  $ cd subdir
  $ hg log -r "getstablerev()"
  [26805aba1e60]: C
  $ cd ..

JSON is also supported:
  $ printf "#!/bin/bash\n\necho '{\"node\": \"D\"}'" > stable.sh
  $ hg log -r "getstablerev()"
  [f585351a92f8]: D

Invalid JSON aborts:
  $ printf "#!/bin/bash\n\necho '{node\": \"D\"}'" > stable.sh
  $ hg log -r "getstablerev()"
  abort: stable rev returned by script (stable.sh) was invalid
  [255]

An alias can be used for simplicity:
  $ printf "#!/bin/bash\n\necho 'A'" > stable.sh
  $ setconfig revsetalias.stable="getstablerev()"
  $ hg log -r stable
  [426bada5c675]: A

Check that stables template keyword works:
  $ cat <<'EOF' > stables.sh
  > #!/bin/bash
  > cat << FOE
  > {
  >   "$1": ["stable1", "stable2"]
  > }
  > FOE
  > EOF
  $ chmod +x stables.sh
  $ setconfig "stablerev.stablesscript=./stables.sh {nodeid}"
  $ hg log -r "D" --template "{stables}\n"
  stable1 stable2

# Auto-pull

Make another repo with "E" (9bc730a19041):
  $ cd ..
  $ newclientrepo
  $ drawdag <<'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg push -q -r 9bc730a19041 --to book --create
  $ cd ../repo1

What if the stable commit isn't present locally?
  $ printf "#!/bin/bash\n\necho '9bc730a19041'" > stable.sh
  $ hg log -r stable
  abort: stable commit (9bc730a19041) not in the repo
  (try hg pull first)
  [255]

The revset can be configured to automatically pull in this case:
  $ setconfig paths.default=test:repo2_server
  $ setconfig stablerev.pullonmissing=True
  $ setconfig remotenames.selectivepulldefault=book
  $ hg log -r stable
  stable commit (9bc730a19041) not in repo; pulling to get it...
  pulling from test:repo2_server
  searching for changes
  [9bc730a19041]: E

But it might not exist even after pulling:
  $ printf "#!/bin/bash\n\necho 'abcdef123'" > stable.sh
  $ hg log -r stable
  stable commit (abcdef123) not in repo; pulling to get it...
  pulling from test:repo2_server
  abort: stable commit (abcdef123) not in the repo
  (try hg pull first)
  [255]

# Targets

Targets are disabled by default:
  $ hg log -r "getstablerev(foo)"
  abort: targets are not supported in this repo
  [255]

But they can be made optional or required:
  $ setconfig stablerev.targetarg=optional
  $ hg log -r "getstablerev(foo)"
  stable commit (abcdef123) not in repo; pulling to get it...
  pulling from test:repo2_server
  abort: stable commit (abcdef123) not in the repo
  (try hg pull first)
  [255]

  $ setconfig stablerev.targetarg=required
  $ hg log -r "getstablerev()"
  abort: must pass a target
  [255]

Try making the script return different locations
  $ cat <<'EOF' > stable.sh
  > #!/bin/bash
  > if [ "$TARGET" = "foo" ]; then
  >    echo 'D'
  > else
  >    echo 'C'
  > fi
  > EOF
  $ hg log -r "getstablerev(foo)"
  [f585351a92f8]: D
  $ hg log -r "getstablerev(bar)"
  [26805aba1e60]: C

Lastly, targets can be used in conjunction with aliases:

  $ setconfig revsetalias.stable="getstablerev(foo)"
  $ hg log -r stable
  [f585351a92f8]: D
