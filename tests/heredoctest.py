import sys

globalvars = {}
localvars = {}
lines = sys.stdin.readlines()
while lines:
    l = lines.pop(0)
    if l.startswith('SALT'):
        print l[:-1]
    elif l.startswith('>>> '):
        snippet = l[4:]
        while lines and lines[0].startswith('... '):
            l = lines.pop(0)
            snippet += "\n" + l[4:]
        c = compile(snippet, '<heredoc>', 'single')
        try:
            exec c in globalvars, localvars
        except Exception, inst:
            print repr(inst)
