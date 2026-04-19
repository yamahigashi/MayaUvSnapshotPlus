# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import os
import sys
import tempfile
import textwrap

from maya import (
    cmds,
    mel,
)
from maya.api import OpenMaya as om

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

EDGE_APPEARANCE_SPECS = [
    ("soft", "Soft Edge", (0.8, 0.8, 0.8), 1),
    ("hard", "Hard Edge", (0.0, 0.75, 1.0), 3),
    ("border", "Border Edge", (1.0, 0.0, 0.0), 6),
    ("boundary", "Boundary Edge", (1.0, 0.0, 0.0), 6),
    ("crease", "Crease Edge", (1.0, 1.0, 0.0), 2),
    ("fold", "Fold Edge", (0.75, 0.75, 0.0), 2),
]
PREVIEW_MAX_DIMENSION = 512
WARNING_COLOR = (1.0, 0.25, 0.25)
WARNING_WIDTH = 4


def _edge_control_name(edge_key, suffix):
    # type: (Text, Text) -> Text
    return "{}Edge{}".format(edge_key, suffix)


def _get_uv_face_polygons(mesh_name, u_min, u_max, v_min, v_max):
    # type: (Text, float, float, float, float) -> List[Any]
    if hasattr(drawer, "get_uv_face_polygons"):
        return drawer.get_uv_face_polygons(mesh_name, u_min, u_max, v_min, v_max)

    # Fallback for stale Maya module caches where ui.py updated before __init__.py.
    fn_mesh = drawer.get_mfnmesh_from_meshlike(mesh_name)
    uv_set_name = fn_mesh.currentUVSetName()
    it_face = om.MItMeshPolygon(fn_mesh.object())
    to_be_map_uv = (u_min != 0.0 or v_min != 0.0 or u_max != 1.0 or v_max != 1.0)
    polygons = []

    while not it_face.isDone():
        us, vs = it_face.getUVs(uv_set_name)
        points = []
        for u, v in zip(us, vs):
            point = (u, v)
            if to_be_map_uv:
                point = drawer.map_uv_into_range(point, u_min, u_max, v_min, v_max)
            points.append(point)

        deduped = []
        for point in points:
            if not deduped or deduped[-1] != point:
                deduped.append(point)
        if len(deduped) >= 3 and deduped[0] == deduped[-1]:
            deduped.pop()
        if len(deduped) >= 3:
            if hasattr(drawer, "UVPolygon"):
                polygons.append(drawer.UVPolygon(deduped))
            else:
                polygons.append({"points": [list(point) for point in deduped]})

        it_face.next()

    return polygons


def _create_edge_appearance_row(edge_key, label, color, width, slider_width):
    # type: (Text, Text, Tuple[float, float, float], int, int) -> None
    row_name = _edge_control_name(edge_key, "AppearanceRow")
    internal_swatch_name = _edge_control_name(edge_key, "InternalColorSwatch")
    draw_internal_name = _edge_control_name(edge_key, "DrawInternal")
    outline_swatch_name = _edge_control_name(edge_key, "OutlineColorSwatch")
    outline_name = _edge_control_name(edge_key, "Outline")
    internal_width_field_name = _edge_control_name(edge_key, "InternalWidthField")
    internal_width_slider_name = _edge_control_name(edge_key, "InternalWidthSlider")
    outline_width_field_name = _edge_control_name(edge_key, "OutlineWidthField")
    outline_width_slider_name = _edge_control_name(edge_key, "OutlineWidthSlider")
    mode_slider_width = max(72, int(slider_width / 2) - 12)

    cmds.rowLayout(
        row_name,
        numberOfColumns=9,
        adjustableColumn=9,
        columnAlign=[
            (1, "right"),
            (2, "left"),
            (3, "left"),
            (4, "left"),
            (5, "left"),
            (6, "left"),
            (7, "left"),
            (8, "left"),
            (9, "left"),
        ],
        columnAttach=[
            (1, "both", 0),
            (2, "both", 0),
            (3, "both", 0),
            (4, "both", 0),
            (5, "both", 0),
            (6, "both", 0),
            (7, "both", 0),
            (8, "both", 0),
            (9, "both", 0),
        ],
        columnWidth=[
            (1, 120),
            (2, 72),
            (3, 24),
            (4, 42),
            (5, mode_slider_width),
            (6, 72),
            (7, 24),
            (8, 42),
            (9, mode_slider_width),
        ],
    )
    cmds.text(label=label + ":", align="right")
    cmds.checkBox(
        draw_internal_name,
        label="Internal",
        value=(edge_key != "fold"),
        changeCommand=lambda *_args, key=edge_key: _on_edge_mode_changed(key),
    )
    cmds.button(
        internal_swatch_name,
        label="",
        height=20,
        width=20,
        backgroundColor=color,
        command=lambda *_args: _pick_edge_color(edge_key, "Internal"),
    )
    cmds.intField(
        internal_width_field_name,
        minValue=1,
        maxValue=100,
        value=width,
        changeCommand=lambda value, key=edge_key: _sync_width_from_field(key, "Internal", value),
    )
    cmds.intSlider(
        internal_width_slider_name,
        min=1,
        max=100,
        value=width,
        step=1,
        dragCommand=lambda value, key=edge_key: _sync_width_from_slider(key, "Internal", value),
        changeCommand=lambda value, key=edge_key: _sync_width_from_slider(key, "Internal", value),
    )
    cmds.checkBox(
        outline_name,
        label="Outline",
        value=(edge_key != "fold"),
        changeCommand=lambda *_args, key=edge_key: _on_edge_mode_changed(key),
    )
    cmds.button(
        outline_swatch_name,
        label="",
        height=20,
        width=20,
        backgroundColor=color,
        command=lambda *_args: _pick_edge_color(edge_key, "Outline"),
    )
    cmds.intField(
        outline_width_field_name,
        minValue=1,
        maxValue=100,
        value=width,
        changeCommand=lambda value, key=edge_key: _sync_width_from_field(key, "Outline", value),
    )
    cmds.intSlider(
        outline_width_slider_name,
        min=1,
        max=100,
        value=width,
        step=1,
        dragCommand=lambda value, key=edge_key: _sync_width_from_slider(key, "Outline", value),
        changeCommand=lambda value, key=edge_key: _sync_width_from_slider(key, "Outline", value),
    )
    cmds.setParent("..")


def _pick_edge_color(edge_key, mode):
    # type: (Text, Text) -> None
    swatch_name = _edge_control_name(edge_key, "{}ColorSwatch".format(mode))
    current_color = cmds.button(swatch_name, query=True, backgroundColor=True)
    cmds.colorEditor(rgbValue=current_color)
    if cmds.colorEditor(query=True, result=True):
        color = cmds.colorEditor(query=True, rgb=True)
        cmds.button(swatch_name, edit=True, backgroundColor=color)
        refresh_preview()


def _get_edge_color(edge_key, mode):
    # type: (Text, Text) -> Tuple[float, float, float]
    return tuple(cmds.button(
        _edge_control_name(edge_key, "{}ColorSwatch".format(mode)),
        query=True,
        backgroundColor=True,
    ))


def _get_edge_width(edge_key, mode):
    # type: (Text, Text) -> int
    return int(cmds.intField(
        _edge_control_name(edge_key, "{}WidthField".format(mode)),
        query=True,
        value=True,
    ))


def _get_draw_internal(edge_key):
    # type: (Text) -> bool
    return cmds.checkBox(
        _edge_control_name(edge_key, "DrawInternal"),
        query=True,
        value=True,
    )


def _get_draw_outline(edge_key):
    # type: (Text) -> bool
    return cmds.checkBox(
        _edge_control_name(edge_key, "Outline"),
        query=True,
        value=True,
    )


def _sync_width_from_slider(edge_key, mode, value):
    # type: (Text, Text, int) -> None
    cmds.intField(
        _edge_control_name(edge_key, "{}WidthField".format(mode)),
        edit=True,
        value=int(value),
    )
    refresh_preview()


def _sync_width_from_field(edge_key, mode, value):
    # type: (Text, Text, int) -> None
    value = max(1, min(100, int(value)))
    cmds.intField(
        _edge_control_name(edge_key, "{}WidthField".format(mode)),
        edit=True,
        value=value,
    )
    cmds.intSlider(
        _edge_control_name(edge_key, "{}WidthSlider".format(mode)),
        edit=True,
        value=value,
    )
    refresh_preview()


def _set_edge_row_visible(edge_key, visible):
    # type: (Text, bool) -> None
    cmds.rowLayout(
        _edge_control_name(edge_key, "AppearanceRow"),
        edit=True,
        visible=visible,
    )


def _set_edge_row_enabled(edge_key, enabled):
    # type: (Text, bool) -> None
    cmds.checkBox(_edge_control_name(edge_key, "DrawInternal"), edit=True, enable=enabled)
    cmds.button(_edge_control_name(edge_key, "InternalColorSwatch"), edit=True, enable=enabled)
    cmds.checkBox(_edge_control_name(edge_key, "Outline"), edit=True, enable=enabled)
    cmds.button(_edge_control_name(edge_key, "OutlineColorSwatch"), edit=True, enable=enabled)
    cmds.intField(_edge_control_name(edge_key, "InternalWidthField"), edit=True, enable=enabled)
    cmds.intSlider(_edge_control_name(edge_key, "InternalWidthSlider"), edit=True, enable=enabled)
    cmds.intField(_edge_control_name(edge_key, "OutlineWidthField"), edit=True, enable=enabled)
    cmds.intSlider(_edge_control_name(edge_key, "OutlineWidthSlider"), edit=True, enable=enabled)


def _get_preview_path():
    # type: () -> Text
    return os.path.join(tempfile.gettempdir(), "uv_snapshot_plus_preview.png")


def _pick_warning_color(*_args):
    # type: (*Any) -> None
    current_color = cmds.button("paddingWarningColorSwatch", query=True, backgroundColor=True)
    cmds.colorEditor(rgbValue=current_color)
    if cmds.colorEditor(query=True, result=True):
        color = cmds.colorEditor(query=True, rgb=True)
        cmds.button("paddingWarningColorSwatch", edit=True, backgroundColor=color)
        refresh_preview()


def _get_warning_color():
    # type: () -> Tuple[float, float, float]
    return tuple(
        cmds.button("paddingWarningColorSwatch", query=True, backgroundColor=True)
    )


def _collect_snapshot_settings():
    # type: () -> Dict[Text, Any]
    return {
        "file_path": cmds.textFieldButtonGrp("filenameField", query=True, text=True),
        "x_resolution": cmds.intSliderGrp("resoX", query=True, value=True),
        "y_resolution": cmds.intSliderGrp("resoY", query=True, value=True),
        "output_mode": cmds.radioButtonGrp("outputModeCtrl", query=True, select=True),
        "fold_angle": cmds.intSliderGrp("foldAngle", query=True, value=True),
        "padding_warning_enabled": cmds.checkBox("paddingWarningEnabled", query=True, value=True),
        "padding_pixels": cmds.intField("paddingPixelsField", query=True, value=True),
        "padding_warning_color": _get_warning_color(),
        "padding_warning_width": cmds.intField("paddingWarningWidthField", query=True, value=True),
        "soft_internal_color": _get_edge_color("soft", "Internal"),
        "hard_internal_color": _get_edge_color("hard", "Internal"),
        "border_internal_color": _get_edge_color("border", "Internal"),
        "boundary_internal_color": _get_edge_color("boundary", "Internal"),
        "crease_internal_color": _get_edge_color("crease", "Internal"),
        "fold_internal_color": _get_edge_color("fold", "Internal"),
        "soft_outline_color": _get_edge_color("soft", "Outline"),
        "hard_outline_color": _get_edge_color("hard", "Outline"),
        "border_outline_color": _get_edge_color("border", "Outline"),
        "boundary_outline_color": _get_edge_color("boundary", "Outline"),
        "crease_outline_color": _get_edge_color("crease", "Outline"),
        "fold_outline_color": _get_edge_color("fold", "Outline"),
        "soft_internal_width": _get_edge_width("soft", "Internal"),
        "hard_internal_width": _get_edge_width("hard", "Internal"),
        "border_internal_width": _get_edge_width("border", "Internal"),
        "boundary_internal_width": _get_edge_width("boundary", "Internal"),
        "crease_internal_width": _get_edge_width("crease", "Internal"),
        "fold_internal_width": _get_edge_width("fold", "Internal"),
        "soft_outline_width": _get_edge_width("soft", "Outline"),
        "hard_outline_width": _get_edge_width("hard", "Outline"),
        "border_outline_width": _get_edge_width("border", "Outline"),
        "boundary_outline_width": _get_edge_width("boundary", "Outline"),
        "crease_outline_width": _get_edge_width("crease", "Outline"),
        "fold_outline_width": _get_edge_width("fold", "Outline"),
        "soft_draw_internal": _get_draw_internal("soft"),
        "hard_draw_internal": _get_draw_internal("hard"),
        "border_draw_internal": _get_draw_internal("border"),
        "boundary_draw_internal": _get_draw_internal("boundary"),
        "crease_draw_internal": _get_draw_internal("crease"),
        "fold_draw_internal": _get_draw_internal("fold"),
        "soft_draw_outline": _get_draw_outline("soft"),
        "hard_draw_outline": _get_draw_outline("hard"),
        "border_draw_outline": _get_draw_outline("border"),
        "boundary_draw_outline": _get_draw_outline("boundary"),
        "crease_draw_outline": _get_draw_outline("crease"),
        "fold_draw_outline": _get_draw_outline("fold"),
        "uv_min_max": get_uv_min_max(),
    }


def _build_drawer_config(settings, width_scale=1.0):
    # type: (Dict[Text, Any], float) -> drawer.EdgeLineDrawerConfig
    config = drawer.EdgeLineDrawerConfig()
    config.update_settings(
        "soft",
        draw_outline=settings["soft_draw_outline"],
        draw_internal=settings["soft_draw_internal"],
        internal_color=settings["soft_internal_color"],
        outline_color=settings["soft_outline_color"],
        internal_width=settings["soft_internal_width"] * width_scale,
        outline_width=settings["soft_outline_width"] * width_scale,
    )
    config.update_settings(
        "hard",
        draw_outline=settings["hard_draw_outline"],
        draw_internal=settings["hard_draw_internal"],
        internal_color=settings["hard_internal_color"],
        outline_color=settings["hard_outline_color"],
        internal_width=settings["hard_internal_width"] * width_scale,
        outline_width=settings["hard_outline_width"] * width_scale,
    )
    config.update_settings(
        "border",
        draw_outline=settings["border_draw_outline"],
        draw_internal=settings["border_draw_internal"],
        internal_color=settings["border_internal_color"],
        outline_color=settings["border_outline_color"],
        internal_width=settings["border_internal_width"] * width_scale,
        outline_width=settings["border_outline_width"] * width_scale,
    )
    config.update_settings(
        "boundary",
        draw_outline=settings["boundary_draw_outline"],
        draw_internal=settings["boundary_draw_internal"],
        internal_color=settings["boundary_internal_color"],
        outline_color=settings["boundary_outline_color"],
        internal_width=settings["boundary_internal_width"] * width_scale,
        outline_width=settings["boundary_outline_width"] * width_scale,
    )
    config.update_settings(
        "crease",
        draw_outline=settings["crease_draw_outline"],
        draw_internal=settings["crease_draw_internal"],
        internal_color=settings["crease_internal_color"],
        outline_color=settings["crease_outline_color"],
        internal_width=settings["crease_internal_width"] * width_scale,
        outline_width=settings["crease_outline_width"] * width_scale,
    )
    config.update_settings(
        "fold",
        draw_outline=settings["fold_draw_outline"],
        draw_internal=settings["fold_draw_internal"],
        internal_color=settings["fold_internal_color"],
        outline_color=settings["fold_outline_color"],
        internal_width=settings["fold_internal_width"] * width_scale,
        outline_width=settings["fold_outline_width"] * width_scale,
        fold_angle=settings["fold_angle"],
    )
    return config


def _build_padding_warning_settings(settings, width_scale=1.0):
    # type: (Dict[Text, Any], float) -> Optional[Dict[Text, Any]]
    if not settings["padding_warning_enabled"]:
        return None

    padding_pixels = max(1.0, float(settings["padding_pixels"]) * width_scale)
    return {
        "enabled": True,
        "padding_pixels": padding_pixels,
        "warning_width": max(1.0, float(settings["padding_warning_width"]) * width_scale),
        "warning_color": [
            int(settings["padding_warning_color"][0] * 255),
            int(settings["padding_warning_color"][1] * 255),
            int(settings["padding_warning_color"][2] * 255),
            255,
        ],
    }


def _build_snapshot_json(settings, width_scale=1.0):
    # type: (Dict[Text, Any], float) -> Tuple[Optional[Text], Optional[Text]]
    mesh = cmds.ls(sl=True, dag=True, type="mesh")
    if not mesh:
        return None, "Select a mesh to preview"

    config = _build_drawer_config(settings, width_scale=width_scale)
    u_min, u_max, v_min, v_max = settings["uv_min_max"]
    tmp_json = []
    tmp_polygons = []
    for mesh_name in mesh:
        edges = drawer.MeshEdges(mesh_name, config)
        draw_info = edges.get_draw_info(u_min, u_max, v_min, v_max)
        tmp_json.extend(list(draw_info.values()))
        tmp_polygons.extend(_get_uv_face_polygons(mesh_name, u_min, u_max, v_min, v_max))

    padding_warning = _build_padding_warning_settings(settings, width_scale=width_scale)
    payload = {
        "edges": tmp_json,
        "polygons": tmp_polygons,
    }
    if padding_warning is not None:
        payload["padding_warning"] = padding_warning

    return drawer.edges_to_json_string(payload), None


def _get_preview_dimensions(settings):
    # type: (Dict[Text, Any]) -> Tuple[int, int]
    width = max(1, int(settings["x_resolution"]))
    height = max(1, int(settings["y_resolution"]))
    scale = float(PREVIEW_MAX_DIMENSION) / float(max(width, height))
    return max(1, int(round(width * scale))), max(1, int(round(height * scale)))


def _set_preview_placeholder(message):
    # type: (Text) -> None
    cmds.image("previewImage", edit=True, visible=False, image="")
    cmds.text("previewStatus", edit=True, visible=True, label=message)


def _set_preview_image(image_path, width, height):
    # type: (Text, int, int) -> None
    cmds.text("previewStatus", edit=True, visible=False)
    cmds.image(
        "previewImage",
        edit=True,
        visible=True,
        image=image_path,
        width=width,
        height=height,
    )


def refresh_preview(*args):
    # type: (*Any) -> None
    settings = _collect_snapshot_settings()
    preview_width, preview_height = _get_preview_dimensions(settings)
    width_scale = float(preview_width) / float(max(1, int(settings["x_resolution"])))
    json_data, error_message = _build_snapshot_json(settings, width_scale=width_scale)
    if error_message:
        _set_preview_placeholder(error_message)
        return

    preview_path = _get_preview_path()

    try:
        drawer.execute_drawer(
            preview_path,
            preview_width,
            preview_height,
            json_data,
            open_after_save=False,
        )
    except Exception as exc:
        _set_preview_placeholder("Preview failed: {}".format(exc))
        return

    _set_preview_image(preview_path, preview_width, preview_height)


def _on_edge_mode_changed(edge_key):
    # type: (Text) -> None
    update_controls()
    refresh_preview()


def _on_uv_area_changed(*args):
    uv_snapchot_ctrl_changed()
    refresh_preview()


def _on_output_mode_changed(*args):
    # type: (*Any) -> None
    update_controls()


def _copy_image_to_clipboard(image_path):
    # type: (Text) -> None
    try:
        from PySide6 import QtGui, QtWidgets  # type: ignore
    except Exception:
        from PySide2 import QtGui, QtWidgets  # type: ignore

    app = QtWidgets.QApplication.instance()
    if app is None:
        raise RuntimeError("Qt application is not available")

    image = QtGui.QImage(image_path)
    if image.isNull():
        raise RuntimeError("Failed to load clipboard image")

    clipboard = app.clipboard()
    clipboard.setImage(image)


def _render_snapshot_to_clipboard(settings, json_data):
    # type: (Dict[Text, Any], Text) -> None
    temp_path = None
    try:
        with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as temp_file:
            temp_path = temp_file.name

        drawer.execute_drawer(
            temp_path,
            settings["x_resolution"],
            settings["y_resolution"],
            json_data,
            open_after_save=False,
        )
        _copy_image_to_clipboard(temp_path)
    finally:
        if temp_path and os.path.exists(temp_path):
            os.unlink(temp_path)


def show_ui():
    # type: () -> None

    gOptionBoxTemplateOffsetText = mel.eval("""$tmp = $gOptionBoxTemplateOffsetText;""")  #  type: int
    gOptionBoxTemplateTextColumnWidth = mel.eval("""$tmp = $gOptionBoxTemplateTextColumnWidth;""")  #  type: int
    # gOptionBoxTemplateSingleWidgetWidth = mel.eval("""$tmp = $gOptionBoxTemplateSingleWidgetWidth;""")  #  type: int
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
                fileFilter='Image Files PNG or SVG(*.png *.svg)'
            )
            if res:
                if not res[0].endswith(".png") and not res[0].endswith(".svg"):
                    res[0] += ".png"
                cmds.optionVar(sv=('uvSnapshotFileName', res[0]))
                cmds.textFieldButtonGrp("filenameField", edit=True, text=res[0])
        """)
    )
    cmds.radioButtonGrp(
        "outputModeCtrl",
        label="Output:",
        labelArray2=("File", "Clipboard"),
        numberOfRadioButtons=2,
        select=1,
        changeCommand=_on_output_mode_changed,
    )
    
    # Size controls
    cmds.intSliderGrp("resoX", label="Size X (px):", field=True, min=1, max=4096, value=2048)  # noqa: E501
    cmds.intSliderGrp("resoY", label="Size Y (px):", field=True, min=1, max=4096, value=2048)  # noqa: E501
    cmds.rowLayout(
        numberOfColumns=6,
        adjustableColumn=2,
        columnAlign=[(1, "right"), (2, "left"), (3, "left"), (4, "left"), (5, "right"), (6, "left")],
        columnWidth=[(1, 120), (2, 100), (3, 72), (4, 24), (5, 42), (6, 48)],
    )
    cmds.checkBox(
        "paddingWarningEnabled",
        label="Padding Warning",
        value=False,
        changeCommand=lambda *_args: (_update_padding_warning_controls(), refresh_preview()),
    )
    cmds.intField(
        "paddingPixelsField",
        minValue=1,
        maxValue=4096,
        value=8,
        changeCommand=lambda *_args: refresh_preview(),
    )
    cmds.button(
        "paddingWarningColorSwatch",
        label="",
        height=20,
        width=20,
        backgroundColor=WARNING_COLOR,
        command=_pick_warning_color,
    )
    cmds.text(label="Width", align="right")
    cmds.intField(
        "paddingWarningWidthField",
        minValue=1,
        maxValue=100,
        value=WARNING_WIDTH,
        changeCommand=lambda *_args: refresh_preview(),
    )
    cmds.setParent("..")
    
    cmds.intSliderGrp("foldAngle", label="Fold Angle", field=True, minValue=0.0, maxValue=360.0, value=60.0)  # noqa: E501

    slider_width = gOptionBoxTemplateTextColumnWidth + gOptionBoxTemplateSliderWidgetWidth + 72
    for edge_key, label, color, width in EDGE_APPEARANCE_SPECS:
        _create_edge_appearance_row(edge_key, label, color, width, slider_width)

    if cmds.about(apiVersion=True) < 20230000:
        cmds.checkBox(_edge_control_name("border", "DrawInternal"), edit=True, value=False)
        cmds.checkBox(_edge_control_name("border", "Outline"), edit=True, value=False)
        _set_edge_row_enabled("border", False)

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

    cmds.frameLayout(label="Preview", collapsable=True, collapse=False)
    cmds.columnLayout(adjustableColumn=True, rowSpacing=6, columnAttach=("both", 8))
    cmds.text("previewStatus", label="Select a mesh to preview", align="center")
    cmds.image("previewImage", visible=False, width=PREVIEW_MAX_DIMENSION, height=PREVIEW_MAX_DIMENSION)
    cmds.setParent("..")
    cmds.setParent("..")

    # Buttons at the bottom
    cmds.button(
        "snapshotActionButton",
        label="Take Snap Shot!",
        command=textwrap.dedent("""
            import uv_snapshot_edge_drawer as drawer
            import uv_snapshot_edge_drawer.ui as ui
            ui.snapshot()
        """)
    )
    cmds.button(label="Close", command='cmds.deleteUI("settingsWindow", window=True)')

    cmds.intSliderGrp("foldAngle", edit=True, changeCommand=refresh_preview)
    cmds.intSliderGrp("resoX", edit=True, changeCommand=refresh_preview)
    cmds.intSliderGrp("resoY", edit=True, changeCommand=refresh_preview)
    cmds.radioButtonGrp("uvAreaTileCtrl", edit=True, changeCommand=_on_uv_area_changed)
    cmds.radioButtonGrp("uvAreaRangeCtrl", edit=True, changeCommand=_on_uv_area_changed)
    cmds.intField("uvAreaTileU", edit=True, changeCommand=refresh_preview)
    cmds.intField("uvAreaTileV", edit=True, changeCommand=refresh_preview)
    cmds.floatSliderGrp("uvSnapshotUMinCtrl", edit=True, changeCommand=refresh_preview)
    cmds.floatSliderGrp("uvSnapshotUMaxCtrl", edit=True, changeCommand=refresh_preview)
    cmds.floatSliderGrp("uvSnapshotVMinCtrl", edit=True, changeCommand=refresh_preview)
    cmds.floatSliderGrp("uvSnapshotVMaxCtrl", edit=True, changeCommand=refresh_preview)

    # Call update_controls initially to set the correct states
    update_controls()

    # Show the window
    uv_snapchot_ctrl_changed()
    cmds.showWindow(settingsWindow)
    refresh_preview()


def update_controls(*args):
    fold_enabled = _get_draw_internal("fold") or _get_draw_outline("fold")
    cmds.intSliderGrp("foldAngle", edit=True, enable=fold_enabled)
    file_mode = cmds.radioButtonGrp("outputModeCtrl", query=True, select=True) == 1
    cmds.textFieldButtonGrp("filenameField", edit=True, enable=file_mode)
    cmds.button(
        "snapshotActionButton",
        edit=True,
        label="Take Snap Shot!" if file_mode else "Copy Snapshot",
    )
    _update_padding_warning_controls()


def _update_padding_warning_controls():
    # type: () -> None
    enabled = cmds.checkBox("paddingWarningEnabled", query=True, value=True)
    cmds.intField("paddingPixelsField", edit=True, enable=enabled)
    cmds.button("paddingWarningColorSwatch", edit=True, enable=enabled)
    cmds.intField("paddingWarningWidthField", edit=True, enable=enabled)


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
    settings = _collect_snapshot_settings()
    json_data, error_message = _build_snapshot_json(settings)
    if error_message:
        cmds.warning("Select some mesh")
        _set_preview_placeholder(error_message)
        return

    if settings["output_mode"] == 1:
        drawer.execute_drawer(
            settings["file_path"],
            settings["x_resolution"],
            settings["y_resolution"],
            json_data,
        )
    else:
        _render_snapshot_to_clipboard(settings, json_data)
    refresh_preview()
    if settings["output_mode"] == 1:
        message = "Exported: {}".format(settings["file_path"])
    else:
        message = "Copied snapshot to clipboard"
    cmds.inViewMessage(
        amg=message,
        pos="topCenter",
        fade=True,
        alpha=0.9,
        fadeStayTime=10000,
        fadeOutTime=1000
    )
