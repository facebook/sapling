import sys, os, getopt

def fancyopts(args, options, state, syntax='', minlen = 0):
    long=[]
    short=''
    map={}
    dt={}

    def help(state, opt, arg, options=options, syntax=syntax):
        print "Usage: ", syntax

        for s, l, d, c in options:
            opt=' '
            if s: opt = opt + '-' + s + ' '
            if l: opt = opt + '--' + l + ' '
            if d: opt = opt + '(' + str(d) + ')'
            print opt
            if c: print '   %s' % c
        sys.exit(0)

    if len(args) < minlen:
        help(state, None, args)

    options=[('h', 'help', help, 'Show usage info')] + options
    
    for s, l, d, c in options:
        map['-'+s] = map['--'+l]=l
        state[l] = d
        dt[l] = type(d)
        if not d is None and not type(d) is type(help): s, l=s+':', l+'='      
        if s: short = short + s
        if l: long.append(l)

    if os.environ.has_key("HG_OPTS"):
        args = os.environ["HG_OPTS"].split() + args

    try:
        opts, args = getopt.getopt(args, short, long)
    except getopt.GetoptError:
        help(state, None, args)
        sys.exit(-1)

    for opt, arg in opts:
        if dt[map[opt]] is type(help): state[map[opt]](state,map[opt],arg)
        elif dt[map[opt]] is type(1): state[map[opt]] = int(arg)
        elif dt[map[opt]] is type(''): state[map[opt]] = arg
        elif dt[map[opt]] is type([]): state[map[opt]].append(arg)
        elif dt[map[opt]] is type(None): state[map[opt]] = 1
        
    return args

