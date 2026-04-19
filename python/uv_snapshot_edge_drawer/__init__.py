# -*- coding: utf-8 -*-
""" Draw edge lines on UV Snapshot images"""
import sys
import math
import json
import tempfile
import subprocess
import os
import time

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
            "soft": {"draw_outline": True, "draw_internal": True, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 1.0, "outline_width": 1.0},
            "hard": {"draw_outline": True, "draw_internal": True, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 3.0, "outline_width": 3.0},
            "fold": {"draw_outline": False, "draw_internal": False, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 2.0, "outline_width": 2.0, "fold_angle": 60.0},
            "crease": {"draw_outline": True, "draw_internal": True, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 3.0, "outline_width": 3.0},
            "border": {"draw_outline": True, "draw_internal": True, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 6.0, "outline_width": 6.0},
            "boundary": {"draw_outline": True, "draw_internal": True, "internal_color": (0.0, 0.0, 0.0), "outline_color": (0.0, 0.0, 0.0), "internal_width": 4.0, "outline_width": 4.0},
        }

    def get_setting(self, key):
        # type: (Text) -> Dict[Text, Any]
        """Get a setting by key"""
        return self.settings.get(key, {})

    def update_settings(self, key, draw_outline=None, draw_internal=None, color=None, internal_color=None, outline_color=None, width=None, internal_width=None, outline_width=None, fold_angle=None):
        # type: (Text, Optional[bool], Optional[bool], Optional[PointLike], Optional[PointLike], Optional[PointLike], Optional[float], Optional[float], Optional[float], Optional[float]) -> None
        """Update a setting by key"""
        if key in self.settings:
            if draw_outline is not None:
                self.settings[key]["draw_outline"] = draw_outline
            if draw_internal is not None:
                self.settings[key]["draw_internal"] = draw_internal
            if color is not None:
                self.settings[key]["internal_color"] = color
                self.settings[key]["outline_color"] = color
            if internal_color is not None:
                self.settings[key]["internal_color"] = internal_color
            if outline_color is not None:
                self.settings[key]["outline_color"] = outline_color
            if width is not None:
                self.settings[key]["internal_width"] = width
                self.settings[key]["outline_width"] = width
            if internal_width is not None:
                self.settings[key]["internal_width"] = internal_width
            if outline_width is not None:
                self.settings[key]["outline_width"] = outline_width

            if fold_angle is not None and key == "fold":
                self.settings["fold"]["fold_angle"] = fold_angle


PROFILE_ENABLED = os.environ.get("MAYA_UV_SNAPSHOT_PROFILE") == "1"
_MESH_TOPOLOGY_CACHE = {}
_MESH_DIRTY_CALLBACKS = {}
DEFAULT_PADDING_WARNING_COLOR = [255, 64, 64, 255]
DEFAULT_PADDING_WARNING_WIDTH = 4.0


def _profile_log(label, started_at):
    # type: (Text, float) -> None
    if PROFILE_ENABLED:
        print("uv_snapshot_edge_drawer: {} {:.4f}s".format(label, time.time() - started_at))


class FoldCandidate(object):
    """Fold edge candidate with precalculated face angle."""

    def __init__(self, angle_radians, lines):
        # type: (float, List[EdgeLine]) -> None
        self.angle_radians = angle_radians
        self.lines = lines


class MeshTopologySnapshot(object):
    """Cacheable topology-derived UV data for a mesh and UV set."""

    def __init__(self, mesh_name, uv_set_name, edge_lines, fold_candidates, polygons):
        # type: (Text, Text, Dict[Text, List[EdgeLine]], List[FoldCandidate], List[UVPolygon]) -> None
        self.mesh_name = mesh_name
        self.uv_set_name = uv_set_name
        self.edge_lines = edge_lines
        self.fold_candidates = fold_candidates
        self.polygons = polygons

    def get_edge_lines(self, fold_angle):
        # type: (float) -> Dict[Text, List[EdgeLine]]
        result = {}
        for key, lines in self.edge_lines.items():
            result[key] = list(lines)

        fold_threshold = math.radians(fold_angle)
        fold_lines = []
        for candidate in self.fold_candidates:
            if candidate.angle_radians > fold_threshold:
                fold_lines.extend(candidate.lines)
        result["fold"] = fold_lines
        return result

    def get_polygons(self, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
        # type: (float, float, float, float) -> List[UVPolygon]
        to_be_map_uv = (umin != 0.0 or vmin != 0.0 or umax != 1.0 or vmax != 1.0)
        if not to_be_map_uv:
            return [UVPolygon([tuple(point) for point in polygon.points]) for polygon in self.polygons]

        mapped = []
        for polygon in self.polygons:
            mapped.append(
                UVPolygon(
                    [
                        map_uv_into_range(point, umin, umax, vmin, vmax)
                        for point in polygon.points
                    ]
                )
            )
        return mapped


class MeshEdges(object):
    """Class to store edge line info for a mesh

    Extract edge line data from Maya mesh objects and generate line information for
    each edge type (soft, hard, fold, etc.) based on the specified settings (EdgeLineDrawerConfig).
    """

    def __init__(self, mesh, config):
        # type: (MeshLike, EdgeLineDrawerConfig) -> None

        self.mesh = get_mfnmesh_from_meshlike(mesh)
        self.config = config
        topology = get_mesh_topology_snapshot(self.mesh)
        self.edge_lines = topology.get_edge_lines(config.get_setting("fold")["fold_angle"])

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
            if not (setting["draw_outline"] or setting["draw_internal"]):
                continue

            internal_color = setting["internal_color"]
            outline_color = setting["outline_color"]
            internal_width = setting["internal_width"]
            outline_width = setting["outline_width"]
            lines = self.edge_lines[key]
            if to_be_map_uv:
                lines = [EdgeLine(
                    line.edge_id,
                    line.map_0_1_into_range(line.uv1, umin, umax, vmin, vmax),
                    line.map_0_1_into_range(line.uv2, umin, umax, vmin, vmax),
                ) for line in lines]

            result[key] = EdgeLineDrawInfo(
                internal_color,
                outline_color,
                internal_width,
                outline_width,
                lines,
                draw_outline=setting.get("draw_outline", True),
                draw_internal=setting.get("draw_internal", True),
            )

        return result


class EdgeLineDrawInfo(object):
    """Class to store edge line info"""

    def __init__(self, internal_color, outline_color, internal_width, outline_width, lines, draw_outline=True, draw_internal=True):
        # type: (Tuple[float, float, float], Tuple[float, float, float], float, float, List[EdgeLine], bool, bool) -> None

        self.internal_color = [
            int(internal_color[0] * 255),
            int(internal_color[1] * 255),
            int(internal_color[2] * 255),
            255
        ]
        self.outline_color = [
            int(outline_color[0] * 255),
            int(outline_color[1] * 255),
            int(outline_color[2] * 255),
            255
        ]
        self.internal_width = internal_width
        self.outline_width = outline_width
        self.draw_outline = draw_outline
        self.draw_internal = draw_internal
        self.lines = lines


class UVPolygon(object):
    """UV face polygon for classification"""

    def __init__(self, points):
        # type: (List[Tuple[float, float]]) -> None
        self.points = [list(point) for point in points]


class DrawerPayloadBuffers(object):
    """Flat payload buffers for the native drawer."""

    def __init__(
        self,
        group_line_offsets,
        line_points,
        group_internal_widths,
        group_outline_widths,
        group_internal_colors,
        group_outline_colors,
        group_draw_outline,
        group_draw_internal,
        polygon_offsets,
        polygon_points,
        padding_warning,
        json_fallback,
    ):
        # type: (List[int], List[float], List[float], List[float], List[int], List[int], List[bool], List[bool], List[int], List[float], Optional[Dict[Text, Any]], Dict[Text, Any]) -> None
        self.group_line_offsets = group_line_offsets
        self.line_points = line_points
        self.group_internal_widths = group_internal_widths
        self.group_outline_widths = group_outline_widths
        self.group_internal_colors = group_internal_colors
        self.group_outline_colors = group_outline_colors
        self.group_draw_outline = group_draw_outline
        self.group_draw_internal = group_draw_internal
        self.polygon_offsets = polygon_offsets
        self.polygon_points = polygon_points
        self.padding_warning = padding_warning
        self._json_fallback = json_fallback
        self._json_string = None

    def as_json_string(self):
        # type: () -> Text
        if self._json_string is None:
            self._json_string = edges_to_json_string(self._json_fallback)
        return self._json_string


def _canonical_line_key(line):
    # type: (EdgeLine) -> Tuple[Tuple[float, float], Tuple[float, float]]
    start = (float(line.uv1[0]), float(line.uv1[1]))
    end = (float(line.uv2[0]), float(line.uv2[1]))
    if start <= end:
        return start, end
    return end, start


def _draw_style_key(group):
    # type: (EdgeLineDrawInfo) -> Tuple[float, float, Tuple[int, int, int, int], Tuple[int, int, int, int], bool, bool]
    return (
        float(group.internal_width),
        float(group.outline_width),
        tuple(int(value) for value in group.internal_color),
        tuple(int(value) for value in group.outline_color),
        bool(group.draw_outline),
        bool(group.draw_internal),
    )


def _merge_payload_edge_groups(groups):
    # type: (List[EdgeLineDrawInfo]) -> List[EdgeLineDrawInfo]
    merged_groups = []
    merged_by_style = {}

    for group in groups:
        style_key = _draw_style_key(group)
        merged = merged_by_style.get(style_key)
        if merged is None:
            merged = EdgeLineDrawInfo(
                (0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0),
                group.internal_width,
                group.outline_width,
                [],
                draw_outline=group.draw_outline,
                draw_internal=group.draw_internal,
            )
            merged.internal_color = list(group.internal_color)
            merged.outline_color = list(group.outline_color)
            merged._seen_lines = set()
            merged_by_style[style_key] = merged
            merged_groups.append(merged)

        for line in group.lines:
            key = _canonical_line_key(line)
            if key in merged._seen_lines:
                continue
            merged._seen_lines.add(key)
            merged.lines.append(line)

    for group in merged_groups:
        del group._seen_lines

    return merged_groups


def _should_draw_edge_type(config, key):
    # type: (EdgeLineDrawerConfig, Text) -> bool
    setting = config.get_setting(key)
    return bool(setting.get("draw_outline") or setting.get("draw_internal"))


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


def map_uv_into_range(uv, u_min, u_max, v_min, v_max):
    # type: (Tuple[float, float]|List[float], float, float, float, float) -> Tuple[float, float]
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
            if isinstance(o, (EdgeLine, EdgeLineDrawInfo, MeshEdges, UVPolygon)):
                return o.__dict__
            return json.JSONEncoder.default(self, o)

    res = json.dumps(edges, cls=EdgeEncoder)

    return res


def build_drawer_payload_buffers(payload):
    # type: (Dict[Text, Any]) -> DrawerPayloadBuffers
    merged_groups = _merge_payload_edge_groups(payload["edges"])
    group_line_offsets = [0]
    line_points = []
    group_internal_widths = []
    group_outline_widths = []
    group_internal_colors = []
    group_outline_colors = []
    group_draw_outline = []
    group_draw_internal = []

    line_count = 0
    for group in merged_groups:
        group_internal_widths.append(float(group.internal_width))
        group_outline_widths.append(float(group.outline_width))
        group_internal_colors.extend(int(value) for value in group.internal_color)
        group_outline_colors.extend(int(value) for value in group.outline_color)
        group_draw_outline.append(bool(group.draw_outline))
        group_draw_internal.append(bool(group.draw_internal))
        for line in group.lines:
            line_points.extend(
                [
                    float(line.uv1[0]),
                    float(line.uv1[1]),
                    float(line.uv2[0]),
                    float(line.uv2[1]),
                ]
            )
            line_count += 1
        group_line_offsets.append(line_count)

    polygon_offsets = [0]
    polygon_points = []
    polygon_point_count = 0
    for polygon in payload["polygons"]:
        for point in polygon.points:
            polygon_points.extend([float(point[0]), float(point[1])])
            polygon_point_count += 1
        polygon_offsets.append(polygon_point_count)

    padding_warning = payload.get("padding_warning")
    optimized_payload = {
        "edges": merged_groups,
        "polygons": payload["polygons"],
        "padding_warning": padding_warning,
    }
    return DrawerPayloadBuffers(
        group_line_offsets=group_line_offsets,
        line_points=line_points,
        group_internal_widths=group_internal_widths,
        group_outline_widths=group_outline_widths,
        group_internal_colors=group_internal_colors,
        group_outline_colors=group_outline_colors,
        group_draw_outline=group_draw_outline,
        group_draw_internal=group_draw_internal,
        polygon_offsets=polygon_offsets,
        polygon_points=polygon_points,
        padding_warning=padding_warning,
        json_fallback=optimized_payload,
    )


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


def _get_mesh_cache_key(fn_mesh):
    # type: (om.MFnMesh) -> Tuple[Text, Text]
    return fn_mesh.fullPathName(), fn_mesh.currentUVSetName()


def _invalidate_mesh_topology_cache(mesh_name):
    # type: (Text) -> None
    stale_keys = [key for key in _MESH_TOPOLOGY_CACHE if key[0] == mesh_name]
    for key in stale_keys:
        _MESH_TOPOLOGY_CACHE.pop(key, None)


def _register_mesh_dirty_callback(fn_mesh):
    # type: (om.MFnMesh) -> None
    mesh_name = fn_mesh.fullPathName()
    if mesh_name in _MESH_DIRTY_CALLBACKS:
        return

    def _on_dirty(*_args):
        _invalidate_mesh_topology_cache(mesh_name)

    callback_id = om.MNodeMessage.addNodeDirtyPlugCallback(fn_mesh.object(), _on_dirty)
    _MESH_DIRTY_CALLBACKS[mesh_name] = callback_id


def clear_mesh_topology_cache():
    # type: () -> None
    for callback_id in list(_MESH_DIRTY_CALLBACKS.values()):
        try:
            om.MMessage.removeCallback(callback_id)
        except RuntimeError:
            pass
    _MESH_DIRTY_CALLBACKS.clear()
    _MESH_TOPOLOGY_CACHE.clear()


def get_mesh_topology_snapshot(meshlike):
    # type: (MeshLike) -> MeshTopologySnapshot
    fn_mesh = get_mfnmesh_from_meshlike(meshlike)
    cache_key = _get_mesh_cache_key(fn_mesh)
    cached = _MESH_TOPOLOGY_CACHE.get(cache_key)
    if cached is not None:
        return cached

    started_at = time.time()
    uv_set_name = fn_mesh.currentUVSetName()
    current_uv_set_id = get_current_uv_set_id(fn_mesh)

    hard_edges_uvs = []
    soft_edges_uvs = []
    border_edges = []
    boundary_edges = []
    crease_edges = []
    fold_candidates = []
    polygons = []

    it_vert = om.MItMeshVertex(fn_mesh.object())
    it_edge = om.MItMeshEdge(fn_mesh.object())
    it_face = om.MItMeshPolygon(fn_mesh.object())

    try:
        crease_ids, _ = fn_mesh.getCreaseEdges()
        for edge_id in crease_ids:
            it_edge.setIndex(edge_id)
            crease_edges.extend(get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name))
    except RuntimeError:
        pass

    if cmds.about(apiVersion=True) >= 20230000:
        border_ids = fn_mesh.getUVBorderEdges(current_uv_set_id)
        for edge_id in border_ids:
            it_edge.setIndex(edge_id)
            border_edges.extend(get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name))

    while not it_edge.isDone():
        lines = get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name)

        if not it_edge.isSmooth:
            hard_edges_uvs.extend(lines)
        else:
            soft_edges_uvs.extend(lines)

        if it_edge.onBoundary():
            boundary_edges.extend(lines)

        if it_edge.numConnectedFaces() >= 2:
            conncetded_ids = it_edge.getConnectedFaces()
            face_id1 = conncetded_ids[0]
            face_id2 = conncetded_ids[1]

            it_face.setIndex(face_id1)
            norm1 = it_face.getNormal()

            it_face.setIndex(face_id2)
            norm2 = it_face.getNormal()

            angle = norm1.angle(norm2)
            candidate_lines = []
            candidate_lines.extend(get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id1))
            candidate_lines.extend(get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id2))
            if candidate_lines:
                fold_candidates.append(FoldCandidate(angle, candidate_lines))

        it_edge.next()

    while not it_face.isDone():
        us, vs = it_face.getUVs(uv_set_name)
        points = []
        for u, v in zip(us, vs):
            points.append((u, v))

        deduped = []
        for point in points:
            if not deduped or deduped[-1] != point:
                deduped.append(point)
        if len(deduped) >= 3 and deduped[0] == deduped[-1]:
            deduped.pop()
        if len(deduped) >= 3:
            polygons.append(UVPolygon(deduped))

        it_face.next()

    snapshot = MeshTopologySnapshot(
        fn_mesh.fullPathName(),
        uv_set_name,
        {
            "hard": hard_edges_uvs,
            "soft": soft_edges_uvs,
            "border": border_edges,
            "boundary": boundary_edges,
            "crease": crease_edges,
            "fold": [],
        },
        fold_candidates,
        polygons,
    )
    _MESH_TOPOLOGY_CACHE[cache_key] = snapshot
    _register_mesh_dirty_callback(fn_mesh)
    _profile_log("mesh topology build {}".format(fn_mesh.fullPathName()), started_at)
    return snapshot


def get_uv_face_polygons(meshlike, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
    # type: (MeshLike, float, float, float, float) -> List[UVPolygon]
    return get_mesh_topology_snapshot(meshlike).get_polygons(umin, umax, vmin, vmax)


def execute_drawer(image_path, width, height, payload_data, open_after_save=True):
    # type: (Text, int, int, Any, bool) -> None
    """Execute the native drawer when available, otherwise fallback to edge_drawer.exe"""

    image_path = _normalize_output_path(image_path)

    if sys.version_info > (3, 0):
        try:
            from uv_snapshot_edge_drawer import _edge_drawer
            if isinstance(payload_data, DrawerPayloadBuffers) and hasattr(_edge_drawer, "draw_edges_buffered"):
                warning = payload_data.padding_warning or {}
                warning_color = warning.get("warning_color", DEFAULT_PADDING_WARNING_COLOR)
                _edge_drawer.draw_edges_buffered(
                    image_path,
                    width,
                    height,
                    payload_data.group_line_offsets,
                    payload_data.line_points,
                    payload_data.group_internal_widths,
                    payload_data.group_outline_widths,
                    payload_data.group_internal_colors,
                    payload_data.group_outline_colors,
                    payload_data.group_draw_outline,
                    payload_data.group_draw_internal,
                    payload_data.polygon_offsets,
                    payload_data.polygon_points,
                    bool(warning.get("enabled", False)),
                    float(warning.get("padding_pixels", 8.0)),
                    float(warning.get("warning_width", DEFAULT_PADDING_WARNING_WIDTH)),
                    [int(value) for value in warning_color],
                )
            else:
                json_data = payload_data.as_json_string() if isinstance(payload_data, DrawerPayloadBuffers) else payload_data
                _edge_drawer.draw_edges(image_path, width, height, json_data)
        except Exception as exc:
            _execute_drawer_cli(image_path, width, height, payload_data, native_error=exc)
            return
    else:
        _execute_drawer_cli(image_path, width, height, payload_data)

    if open_after_save:
        subprocess.call("start " + image_path, shell=True)


def _normalize_output_path(image_path):
    # type: (Text) -> Text
    """Default to PNG when no output extension is provided."""

    _, ext = os.path.splitext(image_path)
    if ext:
        return image_path
    return image_path + ".png"


def _execute_drawer_cli(image_path, width, height, payload_data, native_error=None):
    # type: (Text, int, int, Any, Optional[Exception]) -> None
    """Execute the CLI fallback."""

    temp_path = None
    try:
        json_data = payload_data.as_json_string() if isinstance(payload_data, DrawerPayloadBuffers) else payload_data
        # Avoid Windows command line length limits in the fallback path.
        if len(json_data) > 7500:
            with tempfile.NamedTemporaryFile(mode='w+', delete=False) as temp_file:
                temp_file.write(json_data)
                temp_path = temp_file.name
            json_data = temp_path

        args = [
            "edge_drawer",
            image_path,
            str(width),
            str(height),
            json_data,
        ]

        if sys.version_info > (3, 0):
            result = subprocess.run(args, capture_output=True, text=True)
            if result.returncode != 0:
                if native_error is not None:
                    print("native edge drawer failed: {}".format(native_error))
                print(" ".join(args))
                cmds.error(result.stderr or "edge_drawer.exe error")
                return
        else:
            cmd = subprocess.list2cmdline(args)
            result = subprocess.call(cmd, shell=True)
            if result != 0:
                if native_error is not None:
                    print("native edge drawer failed: {}".format(native_error))
                print(cmd)
                cmds.error("edge_drawer.exe error")
                return
    finally:
        if temp_path and os.path.exists(temp_path):
            os.unlink(temp_path)


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
