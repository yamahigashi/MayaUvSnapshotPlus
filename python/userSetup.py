# # -*- coding: utf-8 -*-
from maya import (
    cmds,
)


def __register_menu():
    """Setup menu"""

    from textwrap import dedent
    cmds.evalDeferred(dedent(
        """
        import uv_snapshot_edge_drawer.startup as startup
        startup.register_menu()
        """
    ))


if __name__ == '__main__':
    try:
        __register_menu()

    except Exception as e:
        # avoidng the "call userSetup.py chain" accidentally stop,
        # all exception must collapse
        import traceback
        traceback.print_exc()
