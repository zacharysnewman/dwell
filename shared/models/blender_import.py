# Blender (headless) half of shared/models/import.mjs (docs/CHARACTERS.md §11, ADR 0070):
# one FBX in, one GLB out. Joins the model's meshes, paints every face one colour of the shared
# palette (the source texture sampled at the face and snapped to the nearest palette stop, or the
# model's `remap` entry), drops the texture and materials, names the clips by their last word
# (`Deer_001_rig|Deer_001_idle` -> `idle`) and writes a GLB with its skin and clips.
#
#   blender -b --factory-startup -P shared/models/blender_import.py -- <job.json>
#
# The job (written by import.mjs): {fbx, texture, out, palette: [[[r, g, b], [r, g, b]], ...] (each
# palette stop and its ramp's base, sRGB 0..1: a colour snaps to the nearest stop and takes its
# ramp's base, the shading being the renderer's),
# remap: [[[r, g, b], [r, g, b]], ...] (from, to; sRGB 0..1), tolerance, frame_step}.
import json
import sys

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
mesh = obj.data

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


uvs = mesh.uv_layers.active.data
col = mesh.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
cache = {}
for poly in mesh.polygons:
    loops = list(poly.loop_indices)
    corners = np.array([uvs[i].uv[:] for i in loops])
    centre = corners.mean(axis=0)
    # The face's colour: the centre and halfway to each corner, the median (a stray texel, such as
    # a watermark's letter, doesn't win).
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
print('IMPORTED', job['out'], len(mesh.vertices), 'verts', [a.name for a in bpy.data.actions])
