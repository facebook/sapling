"""Code for persisting the layout config in various locations.

Basically, if you want to save the layout, this is where you should go
to do it.

"""

import os.path

def layout_to_file(metapath, layout):
    """Save the given layout to a file under the given metapath"""

    layoutfile = os.path.join(metapath, 'layout')
    f = open(layoutfile, 'w')
    f.write(layout)
    f.close()
