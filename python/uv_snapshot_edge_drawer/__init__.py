# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import sys
import math
import json
import tempfile
import subprocess

from maya.api import OpenMaya as om
from maya import (
    cmds,
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
class EdgeLineDrawerConfig:
    """Config for EdgeLineDrawer"""

    def __init__(self):
        self.settings = {
            "soft": {"enabled": True, "color": (0.0, 0.0, 0.0), "width": 1.0},
            "hard": {"enabled": True, "color": (0.0, 0.0, 0.0), "width": 3.0},
            "fold": {"enabled": False, "color": (0.0, 0.0, 0.0), "width": 2.0, "fold_angle": 60.0},
            "crease": {"enabled": True, "color": (0.0, 0.0, 0.0), "width": 3.0},
            "border": {"enabled": True, "color": (0.0, 0.0, 0.0), "width": 6.0},
            "boundary": {"enabled": True, "color": (0.0, 0.0, 0.0), "width": 4.0},
        }

    def get_setting(self, key):
        # type: (Text) -> Dict[Text, Any]
        """Get a setting by key"""
        return self.settings.get(key, {})

    def update_settings(self, key, enabled=None, color=None, width=None, fold_angle=None):
        # type: (Text, Optional[bool], Optional[PointLike], Optional[float], Optional[float]) -> None
        """Update a setting by key"""
        if key in self.settings:
            if enabled is not None:
                self.settings[key]["enabled"] = enabled
            if color is not None:
                self.settings[key]["color"] = color
            if width is not None:
                self.settings[key]["width"] = width

            if fold_angle is not None and key == "fold":
                self.settings["fold"]["fold_angle"] = fold_angle


class MeshEdges(object):
    """Class to store edge line info for a mesh

    Extract edge line data from Maya mesh objects and generate line information for
    each edge type (soft, hard, fold, etc.) based on the specified settings (EdgeLineDrawerConfig).
    """

    def __init__(self, mesh, config):
        # type: (MeshLike, EdgeLineDrawerConfig) -> None

        self.mesh = get_mfnmesh_from_meshlike(mesh)
        self.config = config
        self.edge_lines = get_edge_lines(
                self.mesh,
                soft=config.get_setting("soft").get("enabled", False),
                hard=config.get_setting("hard").get("enabled", False),
                fold=config.get_setting("fold").get("enabled", False),
                crease=config.get_setting("crease").get("enabled", False),
                border=config.get_setting("border").get("enabled", False),
                boundary=config.get_setting("boundary").get("enabled", False),
                fold_angle=config.get_setting("fold")["fold_angle"]
        )

    def get_draw_info(self, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
        # type: (float, float, float, float) -> Dict[Text, EdgeLineDrawInfo]
        """Get edge line draw info for the mesh

        This method converts the mesh's edge lines based on the specified UV coordinate range and
        generates the information necessary for drawing. The edge lines can be mapped to a specific
        UV coordinate range through the umin, vmin, umax, and vmax parameters. This ensures that
        the edge lines are drawn accurately when only part of the mesh is displayed.
        """

        to_be_map_uv = (umin != 0.0 or vmin != 0.0 or umax != 1.0 or vmax != 1.0)

        result = {}
        for key, setting in self.config.settings.items():
            if not setting["enabled"]:
                continue

            color = setting["color"]
            width = setting["width"]
            lines = self.edge_lines[key]
            if to_be_map_uv:
                lines = [EdgeLine(
                    line.edge_id,
                    line.map_0_1_into_range(line.uv1, umin, umax, vmin, vmax),
                    line.map_0_1_into_range(line.uv2, umin, umax, vmin, vmax),
                ) for line in lines]

            result[key] = EdgeLineDrawInfo(color, width, lines)

        return result


class EdgeLineDrawInfo(object):
    """Class to store edge line info"""

    def __init__(self, color, width, lines):
        # type: (Tuple[float, float, float], float, List[EdgeLine]) -> None

        self.line_color = [
            int(color[0] * 255),
            int(color[1] * 255),
            int(color[2] * 255),
            255
        ]
        self.line_width = width
        self.lines = lines


class EdgeLine(object):

    def __init__(self, edge_id, from_uv, to_uv):
        # type: (int, Tuple[float, float], Tuple[float, float]) -> None
        self.edge_id = edge_id
        self.uv1 = list(from_uv)
        self.uv2 = list(to_uv)

    def map_0_1_into_range(self, uv, u_min, u_max, v_min, v_max):
        # type: (Tuple[float, float]|List[float], float, float, float, float) -> Tuple[float, float]
        """Map a UV value from 0-1 into the range of the UV set

        Scenario: no change.
            when u_min = 0.0, u_max = 1.0, v_min = 0.0, v_max = 1.0
            then the UV value is not changed

        Scenario: Wide UV range
            when u_min = -1.0, u_max = 1.0, v_min = -1.0, v_max = 1.0
            then the UV value is mapped into the range of 0-1
            ex. (0.5, 0.5) -> (0.75, 0.75)
        """

        u = uv[0]
        v = uv[1]

        u_range = u_max - u_min
        v_range = v_max - v_min

        u = (u - u_min) / u_range
        v = (v - v_min) / v_range

        return u, v


def edges_to_json_string(edges):
    # type: (List|Dict) -> Text
    """Convert a list or dict of edge lines to a JSON string"""

    class EdgeEncoder(json.JSONEncoder):
        def default(self, o):
            if isinstance(o, (EdgeLine, EdgeLineDrawInfo, MeshEdges)):
                return o.__dict__
            return json.JSONEncoder.default(self, o)

    res = json.dumps(edges, cls=EdgeEncoder)

    return res


##############################################################################
def get_current_uv_set_id(fn_mesh):
    # type: (om.MFnMesh) -> int
    """Get the index of the current UV set"""

    set_names = fn_mesh.getUVSetNames()
    current_set_name = fn_mesh.currentUVSetName()
    current_set_id = set_names.index(current_set_name)

    return current_set_id


def get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id=None):
    # type: (om.MFnMesh, om.MItMeshVertex, om.MItMeshEdge, str, Optional[int]) -> List[EdgeLine]
    """Get the edge line of the current iterators"""

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

    if uv1 == uv2:
        return []

    line = EdgeLine(it_edge.index(), uv1, uv2)

    return [line]


def get_edge_lines(
        meshlike,
        hard=True,
        soft=False,
        border=False,
        boundary=False,
        crease=False,
        fold=False,
        fold_angle=60.0
):
    # type: (MeshLike, bool, bool, bool, bool, bool, bool, float) -> Dict[Text, List[EdgeLine]]
    """Get the edge lines of the given mesh"""

    fn_mesh = get_mfnmesh_from_meshlike(meshlike)

    result = {}
    hard_edges_uvs = []
    soft_edges_uvs = []
    border_edges = []
    boundary_edges = []
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

        if boundary and it_edge.onBoundary():
            lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)
            boundary_edges.extend(lines)

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

    result["hard"] = hard_edges_uvs
    result["soft"] = soft_edges_uvs
    result["border"] = border_edges
    result["boundary"] = boundary_edges
    result["crease"] = crease_edges
    result["fold"] = fold_edges_uvs

    return result


def execute_drawer(image_path, width, height, json_data):
    # type: (Text, int, int, Text) -> None
    """Execute the edge_drawer.exe with the given arguments"""

    # if len(json_data) > 7500:  # windows cmd line length limit is 8191
    if len(json_data) > 1:  # windows cmd line length limit is 8191

        with tempfile.NamedTemporaryFile(mode='w+', delete=False) as temp_file:
            temp_file.write(json_data)
            json_data = temp_file.name

    cmd = " ".join([
        "edge_drawer",
        image_path,
        str(width),
        str(height),
        json_data,
    ])

    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    # stdout = result.stdout
    stderr = result.stderr
    if stderr:
        print(cmd)
        cmds.error(stderr)
        return

    subprocess.call("start " + image_path, shell=True)


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
            "lines": edges["hard"]
        },
        {
            "line_color": [0, 255, 0, 255],
            "line_width": 20.0,
            "lines": edges["fold"]
        }
    ]

    json_data = edges_to_json_string(tmp)

    image_path = r"D:\uea.jpg"
    execute_drawer(image_path, 256, 256, json_data)


# show_ui()
# foo()
