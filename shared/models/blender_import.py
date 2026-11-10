# Blender (headless) half of shared/models/import.mjs (docs/CHARACTERS.md §11, ADR 0070):
# one FBX in, one GLB out. Joins the model's meshes, deletes the parts a `remove` rule names (the
# doe: the deer without its antlers), makes one mesh per model-detail level (Blender's Decimate,
# collapse, to each level's share of the faces; the skin weights carried), paints every face one
# colour of the shared palette (the source texture sampled at the face and snapped to the nearest
# palette stop, or the model's `remap` entry), shades every level flat (a normal a face: the
# low-poly look), drops the texture and materials, names the clips by their last word
# (`Deer_001_rig|Deer_001_idle` -> `idle`) and writes a GLB with its skin and clips, the levels as
# meshes `detail_0`, `detail_1`, ... on the one skin.
#
#   blender -b --factory-startup -P shared/models/blender_import.py -- <job.json>
#
# The job (written by import.mjs): {fbx, texture, out, palette: [[[r, g, b], [r, g, b]], ...] (each
# palette stop and its ramp's base, sRGB 0..1: a colour snaps to the nearest stop and takes its
# ramp's base, the shading being the renderer's),
# remap: [[[r, g, b], [r, g, b]], ...] (from, to; sRGB 0..1), tolerance, frame_step,
# levels: [1, 0.6, ...] (models.json `detail.levels`' ratios, the first the full mesh),
# remove: {joint, above} or null (loose parts weighted wholly to `joint` that reach above `above`
# model units, +z up in Blender, are deleted)}.
import json
import sys

import bmesh
import bpy
import numpy as np

job = json.load(open(sys.argv[sys.argv.index('--') + 1]))

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.fbx(filepath=job['fbx'])

# One mesh: join every mesh object into the first.
meshes = [o for o in bpy.data.objects if o.type == 'MESH']
bpy.ops.object.select_all(action='DESELECT')
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
obj = bpy.context.view_layer.objects.active


def remove_parts(o, rule):
    """Deletes the loose parts weighted wholly to rule['joint'] that reach above rule['above']."""
    g = o.vertex_groups.get(rule['joint'])
    if g is None:
        raise SystemExit(f"remove: no joint {rule['joint']}")
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bm.verts.ensure_lookup_table()
    seen, gone = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        part, stack = [], [v]
        seen.add(v.index)
        while stack:
            a = stack.pop()
            part.append(a)
            for e in a.link_edges:
                b = e.other_vert(a)
                if b.index not in seen:
                    seen.add(b.index)
                    stack.append(b)
        on = all(
            any(x.group == g.index and x.weight > 0.99 for x in o.data.vertices[q.index].groups)
            for q in part
        )
        top = max((o.matrix_world @ q.co).z for q in part)
        if on and top > rule['above']:
            gone.extend(part)
    if not gone:
        raise SystemExit('remove: no part matched')
    bmesh.ops.delete(bm, geom=gone, context='VERTS')
    bm.to_mesh(o.data)
    bm.free()
    print('REMOVED', len(gone), 'verts')


if job.get('remove'):
    remove_parts(obj, job['remove'])

# The source texture, as sRGB floats (h, w, 4).
img = bpy.data.images.load(job['texture'])
w, h = img.size
px = np.empty(w * h * 4, dtype=np.float32)
img.pixels.foreach_get(px)
px = px.reshape(h, w, 4)


def srgb_to_lab(c):
    c = np.asarray(c, dtype=np.float64)
    lin = np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
    m = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]])
    xyz = lin @ m.T / np.array([0.9505, 1.0, 1.089])
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


palette = np.array([p[0] for p in job['palette']], dtype=np.float64)
palette_out = np.array([p[1] for p in job['palette']], dtype=np.float64)
palette_lab = srgb_to_lab(palette)
remap_from = srgb_to_lab(np.array([r[0] for r in job['remap']], dtype=np.float64).reshape(-1, 3))
remap_to = np.array([r[1] for r in job['remap']], dtype=np.float64).reshape(-1, 3)
tolerance = job.get('tolerance', 12.0)


def sample(uv):
    x = int(np.clip(uv[0], 0, 0.9999) * w)
    y = int(np.clip(uv[1], 0, 0.9999) * h)
    return px[y, x, :3]


def snap(c):
    lab = srgb_to_lab(c)
    if len(remap_from):
        d = np.linalg.norm(remap_from - lab, axis=1)
        k = int(np.argmin(d))
        if d[k] <= tolerance:
            return remap_to[k]
    return palette_out[int(np.argmin(np.linalg.norm(palette_lab - lab, axis=1)))]


cache = {}


def paint(mesh):
    """Paints every face one palette colour (COLOR_0), drops materials and UVs, shades it flat."""
    uvs = mesh.uv_layers.active.data
    # A pack's mesh may carry its own (white) vertex colours; ours must be the first and only
    # layer, or the glTF exporter writes it as COLOR_1 and the game, which reads COLOR_0, draws the
    # model white (the chicken and the kitty, 2026-11-10).
    for old in list(mesh.color_attributes):
        mesh.color_attributes.remove(old)
    col = mesh.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    for poly in mesh.polygons:
        loops = list(poly.loop_indices)
        corners = np.array([uvs[i].uv[:] for i in loops])
        centre = corners.mean(axis=0)
        # The face's colour: the centre and halfway to each corner, the median (a stray texel, such
        # as a watermark's letter, doesn't win).
        pts = [centre] + [(centre + c) / 2 for c in corners]
        c = np.median(np.array([sample(p) for p in pts]), axis=0)
        key = tuple(np.round(c, 3))
        if key not in cache:
            cache[key] = snap(c)
        s = cache[key]
        for i in loops:
            col.data[i].color_srgb = (s[0], s[1], s[2], 1.0)
    mesh.color_attributes.active_color = col
    mesh.materials.clear()
    for uv in list(mesh.uv_layers):
        mesh.uv_layers.remove(uv)
    # Flat: one normal a face (the exporter splits the corners; custom normals cleared below).
    for poly in mesh.polygons:
        poly.use_smooth = False


def level(src, ratio, k):
    """A copy of `src` (same armature modifier, parent and vertex groups) decimated to `ratio`."""
    o = src.copy()
    o.data = src.data.copy()
    o.name = o.data.name = f'detail_{k}'
    bpy.context.scene.collection.objects.link(o)
    if ratio < 1:
        d = o.modifiers.new('decimate', 'DECIMATE')
        d.decimate_type = 'COLLAPSE'
        d.ratio = ratio
        d.use_collapse_triangulate = True
        bpy.context.view_layer.objects.active = o
        with bpy.context.temp_override(object=o, active_object=o):
            bpy.ops.object.modifier_move_to_index(modifier='decimate', index=0)
            bpy.ops.object.modifier_apply(modifier='decimate')
    return o


levels = job.get('levels', [1])
outs = [level(obj, r, k) for k, r in enumerate(levels)]
bpy.data.objects.remove(obj)
for o in outs:
    if o.data.has_custom_normals:
        with bpy.context.temp_override(object=o, active_object=o):
            bpy.ops.mesh.customdata_custom_splitnormals_clear()
    paint(o.data)

for a in bpy.data.actions:
    a.name = a.name.split('|')[-1].split('_')[-1].lower()

bpy.ops.export_scene.gltf(
    filepath=job['out'],
    export_format='GLB',
    export_animations=True,
    export_animation_mode='ACTIONS',
    export_frame_step=job.get('frame_step', 2),
    export_force_sampling=True,
    export_optimize_animation_size=True,
    export_materials='NONE',
    export_vertex_color='ACTIVE',
    export_texcoords=False,
    export_tangents=False,
    export_def_bones=True,
    export_skins=True,
    export_morph=False,
    export_yup=True,
    export_image_format="NONE",
    export_active_vertex_color_when_no_material=True,
    export_influence_nb=4,
)
print('IMPORTED', job['out'], [len(o.data.polygons) for o in outs], 'faces', [a.name for a in bpy.data.actions])
