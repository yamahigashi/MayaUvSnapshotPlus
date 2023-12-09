# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import sys
import textwrap

from maya import (
    cmds,
    mel,
)


if sys.version_info > (3, 0):
    from typing import TYPE_CHECKING
    if TYPE_CHECKING:
        from typing import (
            Optional,  # noqa: F401
            Dict,  # noqa: F401
            List,  # noqa: F401
            Tuple,  # noqa: F401
            Pattern,  # noqa: F401
            Callable,  # noqa: F401
            Any,  # noqa: F401
            Text,  # noqa: F401
            Generator,  # noqa: F401
            Union,  # noqa: F401
            Iterable # noqa: F401
        )


##############################################################################
def register_menu():
    # type: () -> None
    """Setup menu"""

    if cmds.about(batch=True):
        return

    # if not is_default_uv_menu_registered():
    if not is_default_window_menu_registered():
        register_default_window_menu()

    cmds.setParent("MayaWindow|mainWindowMenu", menu=True)

    cmds.menuItem(divider=True)
    item = cmds.menuItem(
        "uv_snapshot_edge_line_show_ui",
        label="UV Snapshot Plus",
        annotation="open UV Snapshot Plus window",
        echoCommand=True,
        command=textwrap.dedent(
            """
                import uv_snapshot_edge_drawer.ui as drawer
                drawer.show_ui()
            """)
    )
    print("uv_snapshot_edge_drawer: register menu item as {}".format(item))


def is_default_window_menu_registered():
    # type: () -> bool
    """Check if default Window menu is registered"""
    if not cmds.menu("MayaWindow|mainWindowMenu", exists=True):
        return False

    kids = cmds.menu("MayaWindow|mainWindowMenu", query=True, itemArray=True)
    if not kids:
        return False

    if len(kids) == 0:
        return False

    return True


def register_default_window_menu():
    cmd = '''
    buildViewMenu MayaWindow|mainWindowMenu;
    setParent -menu "MayaWindow|mainWindowMenu";
    '''

    mel.eval(cmd)


def is_default_uv_menu_registered():
    # type: () -> bool
    """Check if default UV menu is registered"""
    if not cmds.menu("MayaWindow|mainUVMenu", exists=True):
        return False

    kids = cmds.menu("MayaWindow|mainUVMenu", query=True, itemArray=True)
    if not kids:
        return False

    if len(kids) == 0:
        return False

    return True


def register_default_uv_menu():
    # FIXME: this is not work
    raise NotImplementedError

    # took from Maya2023/scripts/startup/initMainMenuBar.mel line 781
    cmd = textwrap.dedent("""

		menu -label (uiRes("m_initMainMenuBar.kModelingUV")) -aob true -to true
 			  -postMenuCommandOnce false
			  -familyImage "menuIconPolygons.png"
			  $gMainUVMenu;
    """)
    try:
        mel.eval(cmd)
        print("uv_snapshot_edge_drawer: register default UV menu")

    except Exception:
        import traceback
        traceback.print_exc()
        raise


def deregister_menu():
    # type: () -> None
    """Remove menu"""

    if cmds.about(batch=True):
        return

    try:
        path = "MayaWindow|mainUVMenu|uv_snapshot_edge_line_show_ui"
        cmds.deleteUI(path, menuItem=True)

    except Exception as e:
        import traceback
        traceback.print_exc()
        print(e)
