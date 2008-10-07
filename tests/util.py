import os
import popen2

FIXTURES = os.path.join(os.path.abspath(os.path.dirname(__file__)),
                        'fixtures')

def load_svndump_fixture(path, fixture_name):
    '''Loads an svnadmin dump into a fresh repo at path, which should not
    already exist.
    '''
    os.spawnvp(os.P_WAIT, 'svnadmin', ['svnadmin', 'create', path,])
    proc = popen2.Popen4(['svnadmin', 'load', path,])
    inp = open(os.path.join(FIXTURES, fixture_name))
    proc.tochild.write(inp.read())
    proc.tochild.close()
    proc.wait()
