# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import os
import sys
import math
import json
import tempfile
import subprocess
import textwrap

from maya.api import OpenMaya as om
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
        Point = Tuple[float, float, float]
        PointLike = Union[om.MPoint, om.MVector, Point, List[float]]
        MeshLike = Union[Text, om.MFnMesh, om.MDagPath]


##############################################################################
# 
##############################################################################

class EdgeLine(object):

    def __init__(self, mesh, edge_id, from_uv, to_uv):
        # type: (om.MFnMesh, int, Tuple[float, float], Tuple[float, float]) -> None
        self.mesh_name = mesh.name()
        self.edge_id = edge_id
        self.uv1 = list(from_uv)
        self.uv2 = list(to_uv)


def get_current_uv_set_id(fn_mesh):
    # type: (om.MFnMesh) -> int
    """ メッシュの現在のUVセットIDを取得する"""
    set_names = fn_mesh.getUVSetNames()
    current_set_name = fn_mesh.currentUVSetName()
    current_set_id = set_names.index(current_set_name)

    return current_set_id


def get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id=None):
    # type: (om.MFnMesh, om.MItMeshVertex, om.MItMeshEdge, str, Optional[int]) -> List[EdgeLine]

    if face_id is None:
        res = []
        conncetded_ids = it_edge.getConnectedFaces()
        for face_id in conncetded_ids:
            res.extend(get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id))  # noqa: E501

        return res

    vert1 = it_edge.vertexId(0)
    vert2 = it_edge.vertexId(1)

    it_vert.setIndex(vert1)
    uv1 = it_vert.getUV(face_id, uv_set_name)

    it_vert.setIndex(vert2)
    uv2 = it_vert.getUV(face_id, uv_set_name)

    line = EdgeLine(fn_mesh, it_edge.index(), uv1, uv2)

    return [line]


def get_edge_lines(
        fn_mesh,
        hard=True,
        soft=False,
        border=False,
        crease=False,
        fold=False,
        fold_angle=60.0
):
    # type: (om.MFnMesh, bool, bool, bool, bool, bool, float) -> List[List[EdgeLine]]
    """ メッシュの各種エッジラインを取得する"""

    result = []
    hard_edges_uvs = []
    soft_edges_uvs = []
    border_edges = []
    crease_edges = []
    fold_edges_uvs = []

    uv_set_name = fn_mesh.currentUVSetName()
    current_uv_set_id = get_current_uv_set_id(fn_mesh)

    it_vert = om.MItMeshVertex(fn_mesh.object())
    it_edge = om.MItMeshEdge(fn_mesh.object())
    it_face = om.MItMeshPolygon(fn_mesh.object())

    if crease:
        try:  # noqa: FURB107
            crease_ids, _ = fn_mesh.getCreaseEdges()
            for edge_id in crease_ids:
                it_edge.setIndex(edge_id)
                lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)
                crease_edges.extend(lines)
        except RuntimeError:
            # if no crease edge, maya raise RuntimeError....
            pass

    if border:
        border_ids = fn_mesh.getUVBorderEdges(current_uv_set_id)
        for edge_id in border_ids:
            it_edge.setIndex(edge_id)
            lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)
            border_edges.extend(lines)

    while not it_edge.isDone():

        if hard and not it_edge.isSmooth:
            lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)
            hard_edges_uvs.extend(lines)

        if soft and it_edge.isSmooth:
            lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)
            soft_edges_uvs.extend(lines)

        if fold and it_edge.numConnectedFaces() >= 2:

            # ignore 3 or more connected faces for now.
            conncetded_ids = it_edge.getConnectedFaces()
            face_id1 = conncetded_ids[0]
            face_id2 = conncetded_ids[1]

            it_face.setIndex(face_id1)
            norm1 = it_face.getNormal()

            it_face.setIndex(face_id2)
            norm2 = it_face.getNormal()

            angle = norm1.angle(norm2)

            if angle > math.radians(fold_angle):
                lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id1)
                fold_edges_uvs.extend(lines)
                lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id2)
                fold_edges_uvs.extend(lines)

        it_edge.next()

    result.append(hard_edges_uvs)
    result.append(soft_edges_uvs)
    result.append(border_edges)
    result.append(crease_edges)
    result.append(fold_edges_uvs)

    return result


def edges_to_json_string(edges):
    # type: (Any) -> Text

    class EdgeEncoder(json.JSONEncoder):
        def default(self, o):
            if isinstance(o, EdgeLine):
                return o.__dict__
            return json.JSONEncoder.default(self, o)

    res = json.dumps(edges, cls=EdgeEncoder)

    return res


def execute_drawer(image_path, json_data):
    # type: (Text, Text) -> None
    """ エッジのUV座標を元にエッジラインを描画する"""

    # if len(json_data) > 7500:  # windows cmd line length limit is 8191
    if len(json_data) > 0:  # windows cmd line length limit is 8191

        with tempfile.NamedTemporaryFile(mode='w+', delete=False) as temp_file:
            temp_file.write(json_data)
            json_data = temp_file.name

    cmd = " ".join([
        "edge_drawer",
        image_path,
        json_data,
        "-o",
        image_path,
    ])

    subprocess.call(cmd, shell=True)
    subprocess.call("start " + image_path, shell=True)


def show_ui():
    # type: () -> None

    if cmds.window("settingsWindow", exists=True):
        cmds.deleteUI("settingsWindow", window=True)
    
    settingsWindow = cmds.window("settingsWindow", title="Settings")

    # layout = mel.eval("""getOptionBox();""")
    # cmds.setParent(layout)

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
    cmds.checkBoxGrp("antialias", label="", label1="Anti-alias lines")
    # cmds.checkBoxGrp(label1="Soft Edge", value1=False)
    cmds.checkBoxGrp("exportHardEdge", label="", label1="Hard Edge", value1=True)

    if cmds.about(apiVersion=True) >= 20230000:
        cmds.checkBoxGrp("exportBorderEdge", label="", label1="Border Edge", value1=True)  # noqa: E501
    else:
        cmds.checkBoxGrp("exportBorderEdge", label="", label1="Border Edge", value1=False, enable=False)  # noqa: E501
    cmds.checkBoxGrp("exportCreaseEdge", label="", label1="Crease Edge", value1=True)  # noqa: E501
    cmds.checkBoxGrp("exportFoldEdge", label="", label1="Fold Edge", value1=False)

    cmds.intSliderGrp("foldAngle", label="Fold Angle", field=True, minValue=0.0, maxValue=360.0, value=60.0)  # noqa: E501
    
    # Edge Color controls
    cmds.colorSliderGrp("softEdgeColor", label="Soft Edge Color:", rgb=(0.8, 0.8, 0.8))
    cmds.colorSliderGrp("hardEdgeColor", label="Hard Edge Color:", rgb=(0.0, 0.75, 1.0))
    cmds.colorSliderGrp("borderEdgeColor", label="Border Edge Color:", rgb=(1, 0, 0))
    cmds.colorSliderGrp("creaseEdgeColor", label="Crease Edge Color:", rgb=(1, 1, 0))
    cmds.colorSliderGrp("foldEdgeColor", label="Fold Edge Color:", rgb=(0.75, 0.75, 0))
    cmds.separator(h=10)

    # Edge Width controls
    # cmds.intSliderGrp(label="Soft Edge Line Width:", field=True, min=1, max=100, value=3)  # noqa: E501
    cmds.intSliderGrp("hardEdgeWidth", label="Hard Edge Line Width:", field=True, min=1, max=100, value=3)  # noqa: E501
    cmds.intSliderGrp("borderEdgeWidth", label="Border Edge Line Width:", field=True, min=1, max=100, value=6)  # noqa: E501
    cmds.intSliderGrp("creaseEdgeWidth", label="Crease Edge Line Width:", field=True, min=1, max=100, value=2)  # noqa: E501
    cmds.intSliderGrp("foldEdgeWidth", label="Fold Edge Line Width:", field=True, min=1, max=100, value=2)  # noqa: E501
    cmds.setParent("..")  # End the frameLayout

    # UV Area Settings
    cmds.frameLayout(label="UV Area Settings", collapsable=True)
    cmds.radioButtonGrp("uvAreaType", label="UV Area:", labelArray2=["Tiles", "Range"], numberOfRadioButtons=2)  # noqa: E501
    # cmds.floatFieldGrp(labelArray2=["U:", "V:"], numberOfFields=2, value1=1.0, value2=1.0)  # noqa: E501
    # cmds.floatSliderGrp(label="Range", field=True, minValue=0.0, maxValue=1.0, value=0.5)  # noqa: E501
    cmds.setParent("..")  # End the frameLayout

    # Buttons at the bottom
    cmds.button(
        label="Take Snap Shot!",
        command=textwrap.dedent("""
            import uv_snapshot_edge_drawer as drawer
            drawer.snapshot()
        """)
    )
    cmds.button(label="Close", command='cmds.deleteUI("settingsWindow", window=True)')

    # Show the window
    cmds.showWindow(settingsWindow)


def snapshot():
    mesh = cmds.ls(sl=True, dag=True, type="mesh")
    if not mesh:
        cmds.warning("Select some mesh")
        return

    aa = cmds.checkBoxGrp("antialias", query=True, value1=True)
    entire_uv_range = cmds.radioButtonGrp("uvAreaType", query=True, select=True) == 1
    file_format = "png"
    # uv_set_name = cmds.textFieldGrp("", query=True, text=True)
    file_path = cmds.textFieldButtonGrp("filenameField", query=True, text=True)
    if not file_path.endswith(".png"):
        file_path += ".png"

    overwrite = True
    red_color = cmds.colorSliderGrp("softEdgeColor", query=True, rgbValue=True)[0] * 255  # noqa: E501
    blue_color = cmds.colorSliderGrp("softEdgeColor", query=True, rgbValue=True)[1] * 255  # noqa: E501
    green_color = cmds.colorSliderGrp("softEdgeColor", query=True, rgbValue=True)[2] * 255  # noqa: E501
    u_max = 1.0
    u_min = 0.0
    v_max = 1.0
    v_min = 0.0
    x_resolution = cmds.intSliderGrp("resoX", query=True, value=True)
    y_resolution = cmds.intSliderGrp("resoY", query=True, value=True)

    cmds.uvSnapshot(
        antiAliased=aa,
        entireUVRange=entire_uv_range,
        fileFormat=file_format,
        # uvSetName=uv_set_name,
        name=file_path,
        overwrite=overwrite,
        redColor=red_color,
        blueColor=blue_color,
        greenColor=green_color,
        uMax=u_max,
        uMin=u_min,
        vMax=v_max,
        vMin=v_min,
        xResolution=x_resolution,
        yResolution=y_resolution
    )

    if not os.path.exists(file_path):
        cmds.warning("Snapshot file not found: {}".format(file_path))
        return

    hard_edge = cmds.checkBoxGrp("exportHardEdge", query=True, value1=True)
    border_edge = cmds.checkBoxGrp("exportBorderEdge", query=True, value1=True)
    crease_edge = cmds.checkBoxGrp("exportCreaseEdge", query=True, value1=True)
    fold_edge = cmds.checkBoxGrp("exportFoldEdge", query=True, value1=True)
    fold_angle = cmds.intSliderGrp("foldAngle", query=True, value=True)

    hard_edge_color = cmds.colorSliderGrp("hardEdgeColor", query=True, rgbValue=True)
    border_edge_color = cmds.colorSliderGrp("borderEdgeColor", query=True, rgbValue=True)  # noqa: E501
    crease_edge_color = cmds.colorSliderGrp("creaseEdgeColor", query=True, rgbValue=True)  # noqa: E501
    fold_edge_color = cmds.colorSliderGrp("foldEdgeColor", query=True, rgbValue=True)

    hard_edge_width = cmds.intSliderGrp("hardEdgeWidth", query=True, value=True)
    border_edge_width = cmds.intSliderGrp("borderEdgeWidth", query=True, value=True)
    crease_edge_width = cmds.intSliderGrp("creaseEdgeWidth", query=True, value=True)
    fold_edge_width = cmds.intSliderGrp("foldEdgeWidth", query=True, value=True)

    mesh_fn = get_mfnmesh_from_meshlike(mesh[0])
    edges = get_edge_lines(
            mesh_fn,
            hard=hard_edge,
            border=border_edge,
            crease=crease_edge,
            fold=fold_edge,
            fold_angle=fold_angle
    )

    tmp_json = []
    if fold_edge:
        tmp_json.append({
            "line_color": [
                int(fold_edge_color[0] * 255),
                int(fold_edge_color[1] * 255),
                int(fold_edge_color[2] * 255),
                255
            ],
            "line_width": fold_edge_width,
            "lines": edges[4]
        })
    if hard_edge:
        tmp_json.append({
            "line_color": [
                int(hard_edge_color[0] * 255),
                int(hard_edge_color[1] * 255),
                int(hard_edge_color[2] * 255),
                255
            ],
            "line_width": hard_edge_width,
            "lines": edges[0]
        })
    if crease_edge:
        tmp_json.append({
            "line_color": [
                int(crease_edge_color[0] * 255),
                int(crease_edge_color[1] * 255),
                int(crease_edge_color[2] * 255),
                255
            ],
            "line_width": crease_edge_width,
            "lines": edges[3]
        })
    if border_edge:
        tmp_json.append({
            "line_color": [
                int(border_edge_color[0] * 255),
                int(border_edge_color[1] * 255),
                int(border_edge_color[2] * 255),
                255
            ],
            "line_width": border_edge_width,
            "lines": edges[2]
        })

    json_data = edges_to_json_string(tmp_json)
    execute_drawer(file_path, json_data)
    cmds.inViewMessage(
        amg="Exported: {}".format(file_path),
        pos="topCenter",
        fade=True,
        alpha=0.9,
        fadeStayTime=10000,
        fadeOutTime=1000
    )


def foo():
    """For debug"""
    mesh = cmds.ls(sl=True, dag=True, type="mesh")
    if not mesh:
        cmds.warning("Select some mesh")
        return

    mesh_fn = get_mfnmesh_from_meshlike(mesh[0])
    edges = get_edge_lines(mesh_fn, hard=True, fold=True)

    tmp = [
        {
            "line_color": [255, 0, 0, 255],
            "line_width": 10.0,
            "lines": edges[0]
        },
        {
            "line_color": [0, 255, 0, 255],
            "line_width": 20.0,
            "lines": edges[1]
        }
    ]

    json_data = edges_to_json_string(tmp)

    image_path = r"D:\uea.jpg"
    execute_drawer(image_path, json_data)


##############################################################################
# util
##############################################################################
def as_selection_list(iterable):
    # type: (Iterable) -> om.MSelectionList

    selectionList = om.MSelectionList()
    for each in iterable:
        selectionList.add(each)
    return selectionList


def as_dagpath(name):
    # type: (Text) -> om.MDagPath
    selectionList = as_selection_list([name])

    try:
        return selectionList.getDagPath(0)
    except:  # noqa: E722
        return selectionList.getDependNode(0)


def as_mfnmesh(name):
    # type: (Text) -> om.MFnMesh
    dagpath = as_dagpath(name)
    return om.MFnMesh(dagpath)


def get_mfnmesh_from_meshlike(mesh):
    # type: (MeshLike) -> om.MFnMesh
    """Get MFnMesh from mesh like object."""
    if not isinstance(mesh, om.MFnMesh):
        if not isinstance(mesh, om.MDagPath) and cmds.nodeType(mesh) == "transform":
            mesh = cmds.listRelatives(mesh, shapes=True, fullPath=True)[0]
        mesh = as_mfnmesh(mesh)  # type: ignore

    if not isinstance(mesh, om.MFnMesh):
        raise TypeError("mesh must be MFnMesh or MDagPath")

    return mesh


# show_ui()
# foo()
