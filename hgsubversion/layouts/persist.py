"""Code for persisting the layout config in various locations.

Basically, if you want to save the layout, this is where you should go
to do it.

"""

import os.path

def layout_to_file(meta_data_dir, layout):
    """Save the given layout to a file under the given meta_data_dir"""

    layoutfile = os.path.join(meta_data_dir, 'layout')
    f = open(layoutfile, 'w')
    f.write(layout)
    f.close()
