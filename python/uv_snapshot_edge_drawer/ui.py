# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import sys
import textwrap

from maya import (
    cmds,
    mel,
)

import uv_snapshot_edge_drawer as drawer


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
# 
##############################################################################


def show_ui():
    # type: () -> None

    gOptionBoxTemplateOffsetText = mel.eval("""$tmp = $gOptionBoxTemplateOffsetText;""")  #  type: int
    gOptionBoxTemplateTextColumnWidth = mel.eval("""$tmp = $gOptionBoxTemplateTextColumnWidth;""")  #  type: int
    gOptionBoxTemplateSingleWidgetWidth = mel.eval("""$tmp = $gOptionBoxTemplateSingleWidgetWidth;""")  #  type: int
    gOptionBoxTemplateSliderWidgetWidth= mel.eval("""$tmp = $gOptionBoxTemplateSliderWidgetWidth;""")  #  type: int

    if cmds.window("settingsWindow", exists=True):
        cmds.deleteUI("settingsWindow", window=True)
    
    settingsWindow = cmds.window("settingsWindow", title="Settings")

    cmds.setUITemplate("OptionBoxTemplate", pushTemplate=True)
    cmds.columnLayout("snapUVcol", adjustableColumn=True, rowSpacing=10, columnAttach=("both", 10))
    cmds.frameLayout("snapUVframe", label="Snapshot UVs Settings", collapsable=True, collapse=False)  # noqa: E501
   
    file_name = cmds.optionVar(q="uvSnapshotFileName")
    cmds.textFieldButtonGrp(
        "filenameField",
        label="filename: ",
        placeholderText="output path",
        fileName=file_name,
        adjustableColumn3=2,
        buttonLabel="Browse...",
        buttonCommand=textwrap.dedent("""
            import uv_snapshot_edge_drawer as drawer
            sd = cmds.workspace(q=True, rootDirectory=True)
            res = cmds.fileDialog2(
                fileMode=0,
                caption='Select Output Path',
                okCaption='Select',
                dialogStyle=2,
                startingDirectory=sd,
                fileFilter='*.png'
            )
            if res:
                cmds.optionVar(sv=('uvSnapshotFileName', res[0]))
                cmds.textFieldButtonGrp("filenameField", edit=True, text=res[0])
        """)
    )
    
    # Size controls
    cmds.intSliderGrp("resoX", label="Size X (px):", field=True, min=1, max=4096, value=2048)  # noqa: E501
    cmds.intSliderGrp("resoY", label="Size Y (px):", field=True, min=1, max=4096, value=2048)  # noqa: E501
    
    # Checkboxes
    # cmds.checkBoxGrp(label="", label1="Lock aspect ratio", value1=True)
    cmds.checkBoxGrp("exportSoftEdge", label="", label1="Soft Edge", value1=True)
    cmds.checkBoxGrp("exportHardEdge", label="", label1="Hard Edge", value1=True)

    if cmds.about(apiVersion=True) >= 20230000:
        cmds.checkBoxGrp("exportBorderEdge", label="", label1="UV Border Edge", value1=True)  # noqa: E501
    else:
        cmds.checkBoxGrp("exportBorderEdge", label="", label1="UV Border Edge", value1=False, enable=False)  # noqa: E501
    cmds.checkBoxGrp("exportBoundaryEdge", label="", label1="Mesh Boundary Edge", value1=True)  # noqa: E501
    cmds.checkBoxGrp("exportCreaseEdge", label="", label1="Crease Edge", value1=True)  # noqa: E501
    cmds.checkBoxGrp("exportFoldEdge", label="", label1="Fold Edge", value1=False)

    cmds.intSliderGrp("foldAngle", label="Fold Angle", field=True, minValue=0.0, maxValue=360.0, value=60.0)  # noqa: E501
    
    # Edge Color controls
    cmds.colorSliderGrp("softEdgeColor", label="Soft Edge Color:", rgb=(0.8, 0.8, 0.8))
    cmds.colorSliderGrp("hardEdgeColor", label="Hard Edge Color:", rgb=(0.0, 0.75, 1.0))
    cmds.colorSliderGrp("borderEdgeColor", label="Border Edge Color:", rgb=(1, 0, 0))
    cmds.colorSliderGrp("boundaryEdgeColor", label="Boundary Edge Color:", rgb=(1, 0, 0))
    cmds.colorSliderGrp("creaseEdgeColor", label="Crease Edge Color:", rgb=(1, 1, 0))
    cmds.colorSliderGrp("foldEdgeColor", label="Fold Edge Color:", rgb=(0.75, 0.75, 0))
    cmds.separator(h=10)

    # Edge Width controls
    cmds.intSliderGrp("softEdgeWidth", label="Soft Edge Line Width:", field=True, min=1, max=100, value=1)
    cmds.intSliderGrp("hardEdgeWidth", label="Hard Edge Line Width:", field=True, min=1, max=100, value=3)
    cmds.intSliderGrp("borderEdgeWidth", label="Border Edge Line Width:", field=True, min=1, max=100, value=6)
    cmds.intSliderGrp("boundaryEdgeWidth", label="Boundary Edge Line Width:", field=True, min=1, max=100, value=6)
    cmds.intSliderGrp("creaseEdgeWidth", label="Crease Edge Line Width:", field=True, min=1, max=100, value=2)
    cmds.intSliderGrp("foldEdgeWidth", label="Fold Edge Line Width:", field=True, min=1, max=100, value=2)
    cmds.setParent("..")  # End the frameLayout

    # UV Area Settings
    cmds.frameLayout(label="UV Area Settings", collapsable=True)
    if True:

        cmds.radioButtonGrp(
            "uvAreaTileCtrl",
            label="UV Area:",
            label1="Tiles",
            numberOfRadioButtons=1,
            select=True,
            changeCommand=uv_snapchot_ctrl_changed
        )
        cmds.setUITemplate(popTemplate=True)
        cmds.rowLayout(
            columnAlign4=("right", "center", "right", "center"),
            columnAttach4=("right", "both", "right", "both"),
            columnOffset4=(
                gOptionBoxTemplateOffsetText,
                0,
                gOptionBoxTemplateOffsetText,
                0
            ),
            columnWidth4=(
                gOptionBoxTemplateTextColumnWidth + 36,
                gOptionBoxTemplateSliderWidgetWidth,
                36,
                gOptionBoxTemplateSliderWidgetWidth
            ),
            numberOfColumns=4
        )
        if True:
            cmds.text(label="U:")
            cmds.intField("uvAreaTileU", minValue=1, maxValue=100, value=1)
            cmds.text(label="V:")
            cmds.intField("uvAreaTileV", minValue=1, maxValue=100, value=1)
            cmds.setParent("..")

        cmds.setUITemplate("OptionBoxTemplate", pushTemplate=True)
        cmds.radioButtonGrp(
            "uvAreaRangeCtrl",
            label1="Range:",
            numberOfRadioButtons=1,
            shareCollection="uvAreaTileCtrl",
        )
        cmds.setUITemplate(popTemplate=True)
        if not cmds.uiTemplate("UVSnapshotTemplate", exists=True):
            cmds.uiTemplate("UVSnapshotTemplate")

        cmds.floatSliderGrp(
            defineTemplate="UVSnapshotTemplate",
            minValue=0.0,
            maxValue=1.0,
            field=True,
            fieldMinValue=-10000.0,
            fieldMaxValue=10000.0,
            precision=4,
            sliderStep=0.01,
            columnAttach=(1, "right", gOptionBoxTemplateOffsetText),
            columnAlign3=("right", "left", "left"),
            columnAttach3=("right", "both", "both"),
            columnWidth3=(
                gOptionBoxTemplateTextColumnWidth + 58,
                gOptionBoxTemplateSliderWidgetWidth,
                gOptionBoxTemplateSliderWidgetWidth
            )
        )

        cmds.setUITemplate("UVSnapshotTemplate", pushTemplate=True)
        cmds.floatSliderGrp("uvSnapshotUMinCtrl", label="U Min:")
        cmds.floatSliderGrp("uvSnapshotUMaxCtrl", label="U Max:", value=1.0)
        cmds.floatSliderGrp("uvSnapshotVMinCtrl", label="V Min:")
        cmds.floatSliderGrp("uvSnapshotVMaxCtrl", label="V Max:", value=1.0)

        cmds.setParent("..")  # End the frameLayout

    # Buttons at the bottom
    cmds.button(
        label="Take Snap Shot!",
        command=textwrap.dedent("""
            import uv_snapshot_edge_drawer as drawer
            import uv_snapshot_edge_drawer.ui as ui
            ui.snapshot()
        """)
    )
    cmds.button(label="Close", command='cmds.deleteUI("settingsWindow", window=True)')

    # Show the window
    uv_snapchot_ctrl_changed()
    cmds.showWindow(settingsWindow)


def get_uv_min_max():
    # type: () -> Tuple[float, float, float, float]

    uv_range = cmds.radioButtonGrp("uvAreaTileCtrl", query=True, select=True) == 1
    if uv_range:
        u_min = 0.0
        u_max = float(cmds.intField("uvAreaTileU", query=True, value=True))
        v_min = 0.0
        v_max = float(cmds.intField("uvAreaTileV", query=True, value=True))

    else:
        u_min = cmds.floatSliderGrp("uvSnapshotUMinCtrl", query=True, value=True)
        u_max = cmds.floatSliderGrp("uvSnapshotUMaxCtrl", query=True, value=True)
        v_min = cmds.floatSliderGrp("uvSnapshotVMinCtrl", query=True, value=True)
        v_max = cmds.floatSliderGrp("uvSnapshotVMaxCtrl", query=True, value=True)

    return u_min, u_max, v_min, v_max


def uv_snapchot_ctrl_changed(*args):
    uv_range = cmds.radioButtonGrp("uvAreaTileCtrl", query=True, select=True) == 1
    cmds.intField("uvAreaTileU", edit=True, enable=uv_range)
    cmds.intField("uvAreaTileV", edit=True, enable=uv_range)
    cmds.floatSliderGrp("uvSnapshotUMinCtrl", edit=True, enable=not uv_range)
    cmds.floatSliderGrp("uvSnapshotUMaxCtrl", edit=True, enable=not uv_range)
    cmds.floatSliderGrp("uvSnapshotVMinCtrl", edit=True, enable=not uv_range)
    cmds.floatSliderGrp("uvSnapshotVMaxCtrl", edit=True, enable=not uv_range)


def snapshot():
    mesh = cmds.ls(sl=True, dag=True, type="mesh")
    if not mesh:
        cmds.warning("Select some mesh")
        return

    # entire_uv_range = cmds.radioButtonGrp("uvAreaType", query=True, select=True) == 1
    file_format = "png"
    # uv_set_name = cmds.textFieldGrp("", query=True, text=True)
    file_path = cmds.textFieldButtonGrp("filenameField", query=True, text=True)
    if not file_path.endswith(".png"):
        file_path += ".png"

    overwrite = True
    u_max = 1.0
    u_min = 0.0
    v_max = 1.0
    v_min = 0.0
    x_resolution = cmds.intSliderGrp("resoX", query=True, value=True)
    y_resolution = cmds.intSliderGrp("resoY", query=True, value=True)

    soft_edge = cmds.checkBoxGrp("exportSoftEdge", query=True, value1=True)
    hard_edge = cmds.checkBoxGrp("exportHardEdge", query=True, value1=True)
    border_edge = cmds.checkBoxGrp("exportBorderEdge", query=True, value1=True)
    boundary_edge = cmds.checkBoxGrp("exportBoundaryEdge", query=True, value1=True)
    crease_edge = cmds.checkBoxGrp("exportCreaseEdge", query=True, value1=True)
    fold_edge = cmds.checkBoxGrp("exportFoldEdge", query=True, value1=True)
    fold_angle = cmds.intSliderGrp("foldAngle", query=True, value=True)

    soft_edge_color = cmds.colorSliderGrp("softEdgeColor", query=True, rgbValue=True)
    hard_edge_color = cmds.colorSliderGrp("hardEdgeColor", query=True, rgbValue=True)
    border_edge_color = cmds.colorSliderGrp("borderEdgeColor", query=True, rgbValue=True)
    boundary_edge_color = cmds.colorSliderGrp("boundaryEdgeColor", query=True, rgbValue=True)
    crease_edge_color = cmds.colorSliderGrp("creaseEdgeColor", query=True, rgbValue=True)
    fold_edge_color = cmds.colorSliderGrp("foldEdgeColor", query=True, rgbValue=True)

    soft_edge_width = cmds.intSliderGrp("softEdgeWidth", query=True, value=True)
    hard_edge_width = cmds.intSliderGrp("hardEdgeWidth", query=True, value=True)
    border_edge_width = cmds.intSliderGrp("borderEdgeWidth", query=True, value=True)
    boundary_edge_width = cmds.intSliderGrp("boundaryEdgeWidth", query=True, value=True)
    crease_edge_width = cmds.intSliderGrp("creaseEdgeWidth", query=True, value=True)
    fold_edge_width = cmds.intSliderGrp("foldEdgeWidth", query=True, value=True)

    config = drawer.EdgeLineDrawerConfig()
    config.update_settings("soft", soft_edge, soft_edge_color, soft_edge_width)
    config.update_settings("hard", hard_edge, hard_edge_color, hard_edge_width)
    config.update_settings("border", border_edge, border_edge_color, border_edge_width)
    config.update_settings("boundary", boundary_edge, boundary_edge_color, boundary_edge_width)
    config.update_settings("crease", crease_edge, crease_edge_color, crease_edge_width)
    config.update_settings("fold", fold_edge, fold_edge_color, fold_edge_width, fold_angle)

    edges = drawer.MeshEdges(mesh[0], config)
    u_min, u_max, v_min, v_max = get_uv_min_max()
    draw_info = edges.get_draw_info(u_min, u_max, v_min, v_max)
    tmp_json = list(draw_info.values())

    json_data = drawer.edges_to_json_string(tmp_json)
    drawer.execute_drawer(file_path, x_resolution, y_resolution, json_data)
    cmds.inViewMessage(
        amg="Exported: {}".format(file_path),
        pos="topCenter",
        fade=True,
        alpha=0.9,
        fadeStayTime=10000,
        fadeOutTime=1000
    )
