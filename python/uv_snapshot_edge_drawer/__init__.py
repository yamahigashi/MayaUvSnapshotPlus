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
TOPOLOGY_WARMUP_BATCH_ITEMS = 256
TOPOLOGY_WARMUP_BUDGET_MS = 5.0


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

    def __init__(self, mesh_name, uv_set_name, edge_lines, fold_candidates, polygons=None, polygon_offsets=None, polygon_points=None, build_profile=None, has_edge_data=True, has_polygon_data=True):
        # type: (Text, Text, Dict[Text, List[EdgeLine]], List[FoldCandidate], Optional[List[UVPolygon]], Optional[List[int]], Optional[List[float]], Optional[Dict[Text, float]], bool, bool) -> None
        self.mesh_name = mesh_name
        self.uv_set_name = uv_set_name
        self.edge_lines = edge_lines
        self.fold_candidates = fold_candidates
        self.polygons = polygons
        if polygon_offsets is None:
            polygon_offsets = _polygon_offsets_from_polygons(polygons or [])
        if polygon_points is None:
            polygon_points = _polygon_points_from_polygons(polygons or [])
        self.polygon_offsets = list(polygon_offsets)
        self.polygon_points = list(polygon_points)
        self.build_profile = dict(build_profile or {})
        self.has_edge_data = bool(has_edge_data)
        self.has_polygon_data = bool(has_polygon_data)

    def _ensure_polygons(self):
        # type: () -> List[UVPolygon]
        if self.polygons is None:
            self.polygons = _polygons_from_flat_buffers(self.polygon_offsets, self.polygon_points)
        return self.polygons

    def get_edge_lines(self, fold_angle):
        # type: (float) -> Dict[Text, List[EdgeLine]]
        if not self.has_edge_data:
            return {
                "hard": [],
                "soft": [],
                "border": [],
                "boundary": [],
                "crease": [],
                "fold": [],
            }

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
        if not self.has_polygon_data:
            return []

        to_be_map_uv = (umin != 0.0 or vmin != 0.0 or umax != 1.0 or vmax != 1.0)
        if not to_be_map_uv:
            return self._ensure_polygons()

        mapped = []
        for polygon in self._ensure_polygons():
            mapped.append(
                UVPolygon(
                    [
                        map_uv_into_range(point, umin, umax, vmin, vmax)
                        for point in polygon.points
                    ]
                )
            )
        return mapped

    def get_polygon_buffers(self, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
        # type: (float, float, float, float) -> Tuple[List[int], List[float]]
        if not self.has_polygon_data:
            return [0], []

        to_be_map_uv = (umin != 0.0 or vmin != 0.0 or umax != 1.0 or vmax != 1.0)
        if not to_be_map_uv:
            return self.polygon_offsets, self.polygon_points

        mapped_points = []
        for point_index in range(0, len(self.polygon_points), 2):
            mapped_u, mapped_v = map_uv_into_range(
                (self.polygon_points[point_index], self.polygon_points[point_index + 1]),
                umin,
                umax,
                vmin,
                vmax,
            )
            mapped_points.extend([mapped_u, mapped_v])
        return list(self.polygon_offsets), mapped_points

    def get_draw_info(self, config, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
        # type: (EdgeLineDrawerConfig, float, float, float, float) -> Dict[Text, EdgeLineDrawInfo]
        to_be_map_uv = (umin != 0.0 or vmin != 0.0 or umax != 1.0 or vmax != 1.0)
        edge_lines = self.get_edge_lines(config.get_setting("fold")["fold_angle"])

        result = {}
        for key, setting in config.settings.items():
            if not (setting["draw_outline"] or setting["draw_internal"]):
                continue

            internal_color = setting["internal_color"]
            outline_color = setting["outline_color"]
            internal_width = setting["internal_width"]
            outline_width = setting["outline_width"]
            lines = edge_lines[key]
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


class MeshTopologyBuildSession(object):
    """Incrementally build topology cache on the Maya main thread."""

    def __init__(self, meshlike, include_edges=True, include_polygons=True):
        # type: (MeshLike, bool, bool) -> None
        self.fn_mesh = get_mfnmesh_from_meshlike(meshlike)
        self.include_edges = bool(include_edges)
        self.include_polygons = bool(include_polygons)
        self.cache_key = _get_mesh_cache_key(
            self.fn_mesh,
            include_edges=self.include_edges,
            include_polygons=self.include_polygons,
        )
        self.mesh_name = self.fn_mesh.fullPathName()
        self.started_at = time.time()
        self.done = False

        cached = _MESH_TOPOLOGY_CACHE.get(self.cache_key)
        if cached is not None:
            self.done = True
            return

        self.uv_set_name = self.fn_mesh.currentUVSetName()
        self.current_uv_set_id = get_current_uv_set_id(self.fn_mesh)
        self.hard_edges_uvs = []
        self.soft_edges_uvs = []
        self.border_edges = []
        self.boundary_edges = []
        self.crease_edges = []
        self.fold_candidates = []
        self.polygon_offsets = [0]
        self.polygon_points = []
        if self.include_edges:
            self.phase = "crease"
        elif self.include_polygons:
            self.phase = "faces"
        else:
            self.phase = "finalize"
        self.phase_timings = {
            "crease": 0.0,
            "border": 0.0,
            "edges": 0.0,
            "faces": 0.0,
            "finalize": 0.0,
        }
        self.crease_index = 0
        self.border_index = 0
        self.it_vert = om.MItMeshVertex(self.fn_mesh.object()) if self.include_edges else None
        self.it_edge = om.MItMeshEdge(self.fn_mesh.object()) if self.include_edges else None
        self.it_face_normals = om.MItMeshPolygon(self.fn_mesh.object()) if self.include_edges else None
        self.it_face_polygons = om.MItMeshPolygon(self.fn_mesh.object()) if self.include_polygons else None

        if self.include_edges:
            try:
                crease_ids, _ = self.fn_mesh.getCreaseEdges()
                self.crease_ids = list(crease_ids)
            except RuntimeError:
                self.crease_ids = []

            if cmds.about(apiVersion=True) >= 20230000:
                self.border_ids = list(self.fn_mesh.getUVBorderEdges(self.current_uv_set_id))
            else:
                self.border_ids = []
        else:
            self.crease_ids = []
            self.border_ids = []

    def step(self, max_items=TOPOLOGY_WARMUP_BATCH_ITEMS, max_ms=TOPOLOGY_WARMUP_BUDGET_MS):
        # type: (int, float) -> bool
        if self.done:
            return True

        deadline = time.perf_counter() + (max_ms / 1000.0)
        processed = 0

        while processed < max_items and time.perf_counter() < deadline and not self.done:
            if self.phase == "crease":
                phase_started = time.perf_counter()
                if self.crease_index >= len(self.crease_ids):
                    self.phase_timings["crease"] += time.perf_counter() - phase_started
                    self.phase = "border"
                    continue
                edge_id = self.crease_ids[self.crease_index]
                self.crease_index += 1
                self.it_edge.setIndex(edge_id)
                self.crease_edges.extend(get_current_edge_line(self.fn_mesh, self.it_vert, self.it_edge, self.uv_set_name))
                self.phase_timings["crease"] += time.perf_counter() - phase_started
                processed += 1
                continue

            if self.phase == "border":
                phase_started = time.perf_counter()
                if self.border_index >= len(self.border_ids):
                    self.phase_timings["border"] += time.perf_counter() - phase_started
                    self.phase = "edges"
                    continue
                edge_id = self.border_ids[self.border_index]
                self.border_index += 1
                self.it_edge.setIndex(edge_id)
                self.border_edges.extend(get_current_edge_line(self.fn_mesh, self.it_vert, self.it_edge, self.uv_set_name))
                self.phase_timings["border"] += time.perf_counter() - phase_started
                processed += 1
                continue

            if self.phase == "edges":
                phase_started = time.perf_counter()
                if self.it_edge.isDone():
                    self.phase_timings["edges"] += time.perf_counter() - phase_started
                    self.phase = "faces"
                    continue

                connected_ids = list(self.it_edge.getConnectedFaces())
                face_lines = get_current_edge_lines_for_faces(
                    self.fn_mesh,
                    self.it_vert,
                    self.it_edge,
                    self.uv_set_name,
                    connected_ids,
                )
                lines = [line for _face_id, line in face_lines]
                if not self.it_edge.isSmooth:
                    self.hard_edges_uvs.extend(lines)
                else:
                    self.soft_edges_uvs.extend(lines)

                if self.it_edge.onBoundary():
                    self.boundary_edges.extend(lines)

                if len(face_lines) >= 2:
                    face_id1 = face_lines[0][0]
                    face_id2 = face_lines[1][0]

                    self.it_face_normals.setIndex(face_id1)
                    norm1 = self.it_face_normals.getNormal()

                    self.it_face_normals.setIndex(face_id2)
                    norm2 = self.it_face_normals.getNormal()

                    angle = norm1.angle(norm2)
                    self.fold_candidates.append(
                        FoldCandidate(
                            angle,
                            [face_lines[0][1], face_lines[1][1]],
                        )
                    )

                self.it_edge.next()
                self.phase_timings["edges"] += time.perf_counter() - phase_started
                processed += 1
                continue

            if self.phase == "faces":
                phase_started = time.perf_counter()
                if self.it_face_polygons.isDone():
                    self.phase_timings["faces"] += time.perf_counter() - phase_started
                    self.phase = "finalize"
                    continue
                try:
                    self.polygon_offsets, self.polygon_points = build_polygon_buffers_from_mesh(
                        self.fn_mesh,
                        self.uv_set_name,
                    )
                    self.phase = "finalize"
                except RuntimeError:
                    us, vs = self.it_face_polygons.getUVs(self.uv_set_name)
                    append_deduped_uv_polygon(us, vs, self.polygon_offsets, self.polygon_points)
                    self.it_face_polygons.next()
                self.phase_timings["faces"] += time.perf_counter() - phase_started
                processed += 1
                continue

            phase_started = time.perf_counter()
            self._finalize()
            self.phase_timings["finalize"] += time.perf_counter() - phase_started

        return self.done

    def _finalize(self):
        # type: () -> None
        if self.done:
            return

        snapshot = MeshTopologySnapshot(
            self.mesh_name,
            self.uv_set_name,
            {
                "hard": self.hard_edges_uvs,
                "soft": self.soft_edges_uvs,
                "border": self.border_edges,
                "boundary": self.boundary_edges,
                "crease": self.crease_edges,
                "fold": [],
            },
            self.fold_candidates,
            polygon_offsets=self.polygon_offsets,
            polygon_points=self.polygon_points,
            build_profile=self.phase_timings,
            has_edge_data=self.include_edges,
            has_polygon_data=self.include_polygons,
        )
        _MESH_TOPOLOGY_CACHE[self.cache_key] = snapshot
        _register_mesh_dirty_callback(self.fn_mesh)
        _profile_log("mesh topology build {}".format(self.mesh_name), self.started_at)
        if PROFILE_ENABLED:
            print("uv_snapshot_edge_drawer: mesh topology phases {}".format(json.dumps(self.phase_timings, sort_keys=True)))
        self.done = True


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
        """Get edge line draw info for the mesh."""
        topology = get_mesh_topology_snapshot(self.mesh)
        return topology.get_draw_info(self.config, umin, umax, vmin, vmax)


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
        json_fallback_edges,
    ):
        # type: (List[int], List[float], List[float], List[float], List[int], List[int], List[bool], List[bool], List[int], List[float], Optional[Dict[Text, Any]], List[EdgeLineDrawInfo]) -> None
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
        self._json_fallback_edges = json_fallback_edges
        self._json_string = None

    def as_json_string(self):
        # type: () -> Text
        if self._json_string is None:
            self._json_string = edges_to_json_string(
                {
                    "edges": self._json_fallback_edges,
                    "polygons": _polygons_from_flat_buffers(self.polygon_offsets, self.polygon_points),
                    "padding_warning": self.padding_warning,
                }
            )
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


def dedupe_uv_points(us, vs):
    # type: (Iterable[float], Iterable[float]) -> List[Tuple[float, float]]
    deduped = []
    previous = None
    for u, v in zip(us, vs):
        point = (u, v)
        if previous != point:
            deduped.append(point)
            previous = point
    if len(deduped) >= 3 and deduped[0] == deduped[-1]:
        deduped.pop()
    return deduped


def append_deduped_uv_polygon(us, vs, polygon_offsets, polygon_points):
    # type: (Iterable[float], Iterable[float], List[int], List[float]) -> bool
    deduped_point_count = 0
    previous_u = None
    previous_v = None
    first_u = None
    first_v = None
    start_len = len(polygon_points)

    for u, v in zip(us, vs):
        if previous_u == u and previous_v == v:
            continue
        if deduped_point_count == 0:
            first_u = u
            first_v = v
        polygon_points.extend([float(u), float(v)])
        previous_u = u
        previous_v = v
        deduped_point_count += 1

    if deduped_point_count >= 3 and previous_u == first_u and previous_v == first_v:
        del polygon_points[-2:]
        deduped_point_count -= 1

    if deduped_point_count < 3:
        del polygon_points[start_len:]
        return False

    polygon_offsets.append(polygon_offsets[-1] + deduped_point_count)
    return True


def _append_deduped_uv_polygon_from_id_range(face_uv_ids, start_index, end_index, all_us, all_vs, polygon_offsets, polygon_points):
    # type: (Iterable[int], int, int, Iterable[float], Iterable[float], List[int], List[float]) -> bool
    deduped_point_count = 0
    previous_u = None
    previous_v = None
    first_u = None
    first_v = None
    start_len = len(polygon_points)
    polygon_points_append = polygon_points.append

    for uv_id_index in range(start_index, end_index):
        uv_id = face_uv_ids[uv_id_index]
        u = all_us[uv_id]
        v = all_vs[uv_id]
        if previous_u == u and previous_v == v:
            continue
        if deduped_point_count == 0:
            first_u = u
            first_v = v
        polygon_points_append(u)
        polygon_points_append(v)
        previous_u = u
        previous_v = v
        deduped_point_count += 1

    if deduped_point_count >= 3 and previous_u == first_u and previous_v == first_v:
        del polygon_points[-2:]
        deduped_point_count -= 1

    if deduped_point_count < 3:
        del polygon_points[start_len:]
        return False

    polygon_offsets.append(polygon_offsets[-1] + deduped_point_count)
    return True


def build_polygon_buffers_from_mesh(fn_mesh, uv_set_name):
    # type: (om.MFnMesh, Text) -> Tuple[List[int], List[float]]
    face_uv_counts, face_uv_ids = fn_mesh.getAssignedUVs(uv_set_name)
    if not face_uv_counts:
        return [0], []

    all_us, all_vs = fn_mesh.getUVs(uv_set_name)

    try:
        from uv_snapshot_edge_drawer import _edge_drawer

        if hasattr(_edge_drawer, "build_polygon_buffers"):
            return _edge_drawer.build_polygon_buffers(face_uv_counts, face_uv_ids, all_us, all_vs)
    except (ImportError, AttributeError, RuntimeError, TypeError, ValueError):
        pass

    face_uv_counts = list(face_uv_counts)
    face_uv_ids = list(face_uv_ids)
    all_us = list(all_us)
    all_vs = list(all_vs)

    polygon_offsets = [0]
    polygon_points = []
    uv_index = 0
    for face_uv_count in face_uv_counts:
        face_uv_count = int(face_uv_count)
        if face_uv_count <= 0:
            continue
        next_uv_index = uv_index + face_uv_count
        _append_deduped_uv_polygon_from_id_range(
            face_uv_ids,
            uv_index,
            next_uv_index,
            all_us,
            all_vs,
            polygon_offsets,
            polygon_points,
        )
        uv_index = next_uv_index
    return polygon_offsets, polygon_points


def _polygon_offsets_from_polygons(polygons):
    # type: (List[UVPolygon]) -> List[int]
    polygon_offsets = [0]
    polygon_point_count = 0
    for polygon in polygons:
        polygon_point_count += len(polygon.points)
        polygon_offsets.append(polygon_point_count)
    return polygon_offsets


def _polygon_points_from_polygons(polygons):
    # type: (List[UVPolygon]) -> List[float]
    polygon_points = []
    for polygon in polygons:
        for point in polygon.points:
            polygon_points.extend([float(point[0]), float(point[1])])
    return polygon_points


def _polygons_from_flat_buffers(polygon_offsets, polygon_points):
    # type: (List[int], List[float]) -> List[UVPolygon]
    polygons = []
    for polygon_index in range(max(0, len(polygon_offsets) - 1)):
        start = polygon_offsets[polygon_index]
        end = polygon_offsets[polygon_index + 1]
        points = []
        for point_index in range(start, end):
            base_index = point_index * 2
            points.append((polygon_points[base_index], polygon_points[base_index + 1]))
        polygons.append(UVPolygon(points))
    return polygons


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

    polygon_offsets = payload.get("polygon_offsets")
    polygon_points = payload.get("polygon_points")
    if polygon_offsets is None or polygon_points is None:
        polygon_offsets = [0]
        polygon_points = []
        polygon_point_count = 0
        for polygon in payload["polygons"]:
            for point in polygon.points:
                polygon_points.extend([float(point[0]), float(point[1])])
                polygon_point_count += 1
            polygon_offsets.append(polygon_point_count)

    padding_warning = payload.get("padding_warning")
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
        json_fallback_edges=merged_groups,
    )


##############################################################################
def get_current_uv_set_id(fn_mesh):
    # type: (om.MFnMesh) -> int
    """Get the index of the current UV set"""

    set_names = fn_mesh.getUVSetNames()
    current_set_name = fn_mesh.currentUVSetName()
    current_set_id = set_names.index(current_set_name)

    return current_set_id


def _get_current_edge_line_for_face(it_vert, it_edge, uv_set_name, face_id):
    # type: (om.MItMeshVertex, om.MItMeshEdge, str, int) -> Optional[EdgeLine]
    vert1 = it_edge.vertexId(0)
    vert2 = it_edge.vertexId(1)

    it_vert.setIndex(vert1)
    uv1 = it_vert.getUV(face_id, uv_set_name)

    it_vert.setIndex(vert2)
    uv2 = it_vert.getUV(face_id, uv_set_name)

    if uv1 == uv2:
        return None

    return EdgeLine(it_edge.index(), uv1, uv2)


def get_current_edge_lines_for_faces(fn_mesh, it_vert, it_edge, uv_set_name, face_ids):
    # type: (om.MFnMesh, om.MItMeshVertex, om.MItMeshEdge, str, Iterable[int]) -> List[Tuple[int, EdgeLine]]
    del fn_mesh
    lines = []
    for face_id in face_ids:
        line = _get_current_edge_line_for_face(it_vert, it_edge, uv_set_name, int(face_id))
        if line is not None:
            lines.append((int(face_id), line))
    return lines


def get_current_edge_line(fn_mesh, it_vert, it_edge, uv_set_name, face_id=None):
    # type: (om.MFnMesh, om.MItMeshVertex, om.MItMeshEdge, str, Optional[int]) -> List[EdgeLine]
    """Get the edge line of the current iterators"""

    if face_id is None:
        return [line for _face_id, line in get_current_edge_lines_for_faces(fn_mesh, it_vert, it_edge, uv_set_name, it_edge.getConnectedFaces())]

    line = _get_current_edge_line_for_face(it_vert, it_edge, uv_set_name, face_id)
    if line is None:
        return []
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


def _get_mesh_cache_key(fn_mesh, include_edges=True, include_polygons=True):
    # type: (om.MFnMesh, bool, bool) -> Tuple[Text, Text, bool, bool]
    return (
        fn_mesh.fullPathName(),
        fn_mesh.currentUVSetName(),
        bool(include_edges),
        bool(include_polygons),
    )


def _invalidate_mesh_topology_cache(mesh_name):
    # type: (Text) -> None
    stale_keys = [key for key in _MESH_TOPOLOGY_CACHE if key[0] == mesh_name]
    for key in stale_keys:
        _MESH_TOPOLOGY_CACHE.pop(key, None)


def get_cached_mesh_topology_snapshot(meshlike, include_edges=True, include_polygons=True):
    # type: (MeshLike, bool, bool) -> Optional[MeshTopologySnapshot]
    fn_mesh = get_mfnmesh_from_meshlike(meshlike)
    return _MESH_TOPOLOGY_CACHE.get(
        _get_mesh_cache_key(
            fn_mesh,
            include_edges=include_edges,
            include_polygons=include_polygons,
        )
    )


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


def start_mesh_topology_build_session(meshlike, include_edges=True, include_polygons=True):
    # type: (MeshLike, bool, bool) -> MeshTopologyBuildSession
    return MeshTopologyBuildSession(
        meshlike,
        include_edges=include_edges,
        include_polygons=include_polygons,
    )


def get_mesh_topology_snapshot(meshlike, include_edges=True, include_polygons=True):
    # type: (MeshLike, bool, bool) -> MeshTopologySnapshot
    fn_mesh = get_mfnmesh_from_meshlike(meshlike)
    cache_key = _get_mesh_cache_key(
        fn_mesh,
        include_edges=include_edges,
        include_polygons=include_polygons,
    )
    cached = _MESH_TOPOLOGY_CACHE.get(cache_key)
    if cached is not None:
        return cached
    session = start_mesh_topology_build_session(
        fn_mesh,
        include_edges=include_edges,
        include_polygons=include_polygons,
    )
    while not session.step(max_items=TOPOLOGY_WARMUP_BATCH_ITEMS, max_ms=TOPOLOGY_WARMUP_BUDGET_MS):
        pass
    return _MESH_TOPOLOGY_CACHE[cache_key]


def get_uv_face_polygons(meshlike, umin=0.0, umax=1.0, vmin=0.0, vmax=1.0):
    # type: (MeshLike, float, float, float, float) -> List[UVPolygon]
    return get_mesh_topology_snapshot(
        meshlike,
        include_edges=False,
        include_polygons=True,
    ).get_polygons(umin, umax, vmin, vmax)


def render_payload_to_path(image_path, width, height, payload_data):
    # type: (Text, int, int, Any) -> Text
    """Render payload data to a path without any UI side effects."""
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
    else:
        _execute_drawer_cli(image_path, width, height, payload_data)

    return image_path


def execute_drawer(image_path, width, height, payload_data, open_after_save=True):
    # type: (Text, int, int, Any, bool) -> None
    """Execute the native drawer when available, otherwise fallback to edge_drawer.exe"""

    image_path = render_payload_to_path(image_path, width, height, payload_data)
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
                raise RuntimeError(result.stderr or "edge_drawer.exe error")
        else:
            cmd = subprocess.list2cmdline(args)
            result = subprocess.call(cmd, shell=True)
            if result != 0:
                if native_error is not None:
                    print("native edge drawer failed: {}".format(native_error))
                print(cmd)
                raise RuntimeError("edge_drawer.exe error")
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
