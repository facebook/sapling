  $ configure modernclient
  $ newclientrepo
  $ enable stablerev
  $ sl debugdrawdag <<'EOS'
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
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > EOF
  $ chmod +x stable.py
  $ setconfig stablerev.script="python stable.py"
  $ sl log -r "getstablerev()" --debug
  Executing script: python stable.py
  setting current working directory to: $TESTTMP/repo1
  script stdout:
  
  abort: stable rev returned by script (python stable.py) was empty
  [255]

Make the script return something:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print("B")
  > EOF
  $ sl log -r "getstablerev()"
  [112478962961]: B

Change the script, change the result:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print("C")
  > EOF
  $ sl log -r "getstablerev()"
  [26805aba1e60]: C

The script is always run relative to repo root:
  $ mkdir subdir
  $ cd subdir
  $ sl log -r "getstablerev()"
  [26805aba1e60]: C
  $ cd ..

JSON is also supported:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print('{\"node\": \"D\"}')
  > EOF
  $ sl log -r "getstablerev()"
  [f585351a92f8]: D

Invalid JSON aborts:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print('{node\": \"D\"}')
  > EOF
  $ sl log -r "getstablerev()"
  abort: stable rev returned by script (python stable.py) was invalid
  [255]

An alias can be used for simplicity:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print("A")
  > EOF
  $ setconfig revsetalias.stable="getstablerev()"
  $ sl log -r stable
  [426bada5c675]: A

Check that stables template keyword works:
  $ cat << 'EOF' > stables.py
  > #!/usr/bin/env python
  > import sys
  > print('{{\"{nodeid}\": [\"stable1\",\"stable2\"]}}'.format(nodeid=sys.argv[1]))
  > EOF
  $ chmod +x stables.py
  $ setconfig "stablerev.stablesscript=python stables.py {nodeid}"
  $ sl log -r "D" --template "{stables}"
  stable1 stable2 (no-eol)

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
  $ sl push -q -r 9bc730a19041 --to book --create
  $ cd ../repo1

What if the stable commit isn't present locally?
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print("9bc730a19041")
  > EOF
  $ sl log -r stable
  abort: stable commit (9bc730a19041) not in the repo
  (try sl pull first)
  [255]

The revset can be configured to automatically pull in this case:
  $ setconfig paths.default=test:repo2_server
  $ setconfig stablerev.pullonmissing=True
  $ setconfig remotenames.selectivepulldefault=book
  $ sl log -r stable
  stable commit (9bc730a19041) not in repo; pulling to get it...
  pulling from test:repo2_server
  searching for changes
  [9bc730a19041]: E

But it might not exist even after pulling:
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > print("abcdef123")
  > EOF
  $ sl log -r stable
  stable commit (abcdef123) not in repo; pulling to get it...
  pulling from test:repo2_server
  abort: stable commit (abcdef123) not in the repo
  (try sl pull first)
  [255]

# Targets

Targets are disabled by default:
  $ sl log -r "getstablerev(foo)"
  abort: targets are not supported in this repo
  [255]

But they can be made optional or required:
  $ setconfig stablerev.targetarg=optional
  $ sl log -r "getstablerev(foo)"
  stable commit (abcdef123) not in repo; pulling to get it...
  pulling from test:repo2_server
  abort: stable commit (abcdef123) not in the repo
  (try sl pull first)
  [255]

  $ setconfig stablerev.targetarg=required
  $ sl log -r "getstablerev()"
  abort: must pass a target
  [255]

Try making the script return different locations
  $ cat << 'EOF' > stable.py
  > #!/usr/bin/env python
  > import os
  > if os.getenv("TARGET") == "foo":
  >   print('D')
  > else:
  >   print('C')
  > EOF
  $ sl log -r "getstablerev(foo)"
  [f585351a92f8]: D
  $ sl log -r "getstablerev(bar)"
  [26805aba1e60]: C

Lastly, targets can be used in conjunction with aliases:

  $ setconfig revsetalias.stable="getstablerev(foo)"
  $ sl log -r stable
  [f585351a92f8]: D
