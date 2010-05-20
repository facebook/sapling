#/bin/sh

hideport() { sed "s/localhost:$HGPORT/localhost:\$HGPORT/"; }

repr() { python -c "import sys; print repr(sys.stdin.read()).replace('\\n', '\n')"; }

hidehex() { python -c 'import sys, re; print re.replace("\b[0-9A-Fa-f]{12,40}", "X" * 12)'; }

hidetmp() { sed "s/$HGTMP/\$HGTMP/"; }

hidebackup() { sed 's/\(saving bundle to \).*/\1/'; }

cleanrebase() {
    sed -e 's/\(Rebase status stored to\).*/\1/'  \
        -e 's/\(Rebase status restored from\).*/\1/' \
        -e 's/\(saving bundle to \).*/\1/';
}
