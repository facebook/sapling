#require git no-windows

  $ eagerepo
  $ enable tweakdefaults
  $ setconfig tweakdefaults.logdefaultfollow=True

Build a Git repo with a subtree URL that tries to inject an extension into
the cached Git repo config.

  $ . $TESTDIR/git.sh
  $ mkdir evil
  $ cd evil
  $ git init -q .
  $ git config core.autocrlf false
  $ echo "# Totally normal project" > README.md
  $ mkdir src
  $ echo "print('hello')" > src/main.py
  $ git add -A
  $ git commit -qm "initial commit"
  $ git branch -M main
  $ TREE=$(git rev-parse HEAD^{tree})
  $ cat > makecommit.py <<'PY'
  > import base64
  > import json
  > import sys
  > tree = sys.argv[1]
  > payload = base64.b64encode(
  >     b"from pathlib import Path\n"
  >     b"Path('../sapling-rce-proof').write_text('executed\\n')\n"
  > ).decode()
  > url = "https://example.invalid/x\n[extensions]\nzz = python-base64:%s\n" % payload
  > imports = [
  >     {
  >         "url": url,
  >         "from_commit": "0" * 40,
  >         "from_path": path,
  >         "to_path": path,
  >     }
  >     for path in ("README.md", "src")
  > ]
  > print("tree %s" % tree)
  > print("author a <a@b.c> 0 +0000")
  > print("committer a <a@b.c> 0 +0000")
  > metadata = [{"v": 1, "imports": imports}]
  > print("subtree %s" % json.dumps(metadata, separators=(",", ":")))
  > print("")
  > print("initial commit")
  > PY
  $ $PYTHON makecommit.py "$TREE" > ../commit.txt
  $ C=$(git hash-object -w -t commit --stdin < ../commit.txt)
  $ git update-ref refs/heads/main "$C"
  $ cd ..
  $ git init -q --bare evil.git
  $ git --git-dir=evil.git symbolic-ref HEAD refs/heads/main
  $ cd evil
  $ git -c push.negotiate=false push -qf ../evil.git main
  $ cd ..

Reading file history rejects the hostile subtree URL before any config injection
can execute.

  $ sl clone -q --git "$TESTTMP/evil.git" victim
  $ sl --cwd victim log README.md
  commit:      * (glob)
  bookmark:    remote/main
  hoistedname: main
  user:        a <a@b.c>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit
  
  abort: subtree URL 'https://example.invalid/x\n[extensions]\nzz = python-base64:*\n' contains control character '\n' (glob)
  [255]
  $ test ! -e sapling-rce-proof
