import os
import shutil

svn_subcommands = { }

def register_subcommand(name):
    def inner(fn):
        svn_subcommands[name] = fn
        return fn
    return inner


def wipe_all_files(hg_wc_path):
    files = [f for f in os.listdir(hg_wc_path) if f != '.hg']
    for f in files:
        f = os.path.join(hg_wc_path, f)
        if os.path.isdir(f):
            shutil.rmtree(f)
        else:
            os.remove(f)


def remove_all_files_with_status(path, rev_paths, strip_path, status):
    for p in rev_paths:
        if rev_paths[p].action == status:
            if p.startswith(strip_path):
                fi = p[len(strip_path)+1:]
                if len(fi) > 0:
                    fi = os.path.join(path, fi)
                    if os.path.isfile(fi):
                        os.remove(fi)
                        print 'D %s' % fi
                    elif os.path.isdir(fi):
                        shutil.rmtree(fi)
                        print 'D %s' % fi
