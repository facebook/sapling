import sys, os, getopt

def fancyopts(args, options, state, syntax='', minlen = 0):
    long=[]
    short=''
    map={}
    dt={}

    for s, l, d, c in options:
        map['-'+s] = map['--'+l]=l
        state[l] = d
        dt[l] = type(d)
        if not d is None and not callable(d): s, l=s+':', l+'='
        if s: short = short + s
        if l: long.append(l)

    if os.environ.has_key("HG_OPTS"):
        args = os.environ["HG_OPTS"].split() + args

    opts, args = getopt.getopt(args, short, long)

    for opt, arg in opts:
        if dt[map[opt]] is type(fancyopts): state[map[opt]](state,map[opt],arg)
        elif dt[map[opt]] is type(1): state[map[opt]] = int(arg)
        elif dt[map[opt]] is type(''): state[map[opt]] = arg
        elif dt[map[opt]] is type([]): state[map[opt]].append(arg)
        elif dt[map[opt]] is type(None): state[map[opt]] = 1

    return args

