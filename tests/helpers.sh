#/bin/sh

hideport() { sed "s/localhost:$HGPORT/localhost:\$HGPORT/"; }

repr() { python -c "import sys; print repr(sys.stdin.read()).replace('\\n', '\n')" }

hidehex() { python -c 'import sys, re; print re.replace("\b[0-9A-Fa-f]{12,40}", "X" * 12)' }

hidetmp() { sed "s/$HGTMP/\$HGTMP/"; }