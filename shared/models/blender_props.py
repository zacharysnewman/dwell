# Blender (headless) half of shared/models/import.mjs for the static props (shared/models/props.json,
# ADR 0070 §props): many FBX in one run, one GLB each. Per prop: joins its meshes, applies the
# transforms, scales it so its longest side is the data's `length` (metres), puts its origin at its
# bounds' centre (a hand tool's grip is the data's `grip`, from there), paints every face one colour
# of the shared palette (the pack's gradient texture sampled at the face, snapped to the nearest
# palette stop's ramp base, as blender_import.py), removes the pack's own colour layers first (the
# chicken bug: a pack's white COLOR_0 pushed ours to COLOR_1) and writes a GLB with no skin, clip,
# texture or material.
#
#   blender -b --factory-startup -P shared/models/blender_props.py -- <job.json>
#
# The job (written by import.mjs): {texture, palette: [[[r,g,b],[r,g,b]], ...],
# props: [{fbx, out, length}, ...]}.
import json
import sys

import bpy
import numpy as np
from mathutils import Vector

job = json.load(open(sys.argv[sys.argv.index('--') + 1]))

img = bpy.data.images.load(job['texture'])
w, h = img.size
px = np.empty(w * h * 4, dtype=np.float32)
img.pixels.foreach_get(px)
px = px.reshape(h, w, 4)[:, :, :3].copy()


def srgb_to_lab(c):
    c = np.asarray(c, dtype=np.float64)
    lin = np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
    m = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]])
    xyz = lin @ m.T / np.array([0.9505, 1.0, 1.089])
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


palette_lab = srgb_to_lab(np.array([p[0] for p in job['palette']], dtype=np.float64))
palette_out = np.array([p[1] for p in job['palette']], dtype=np.float64)


def sample(uv):
    x = int(np.clip(uv[0], 0, 0.9999) * w)
    y = int(np.clip(uv[1], 0, 0.9999) * h)
    return px[y, x]


def snap(c):
    return palette_out[int(np.argmin(np.linalg.norm(palette_lab - srgb_to_lab(c), axis=1)))]


for p in job['props']:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=p['fbx'])
    meshes = [o for o in bpy.data.objects if o.type == 'MESH']
    bpy.ops.object.select_all(action='DESELECT')
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    # Unparent (keeping the world transform) and bake every transform into the vertices.
    bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM')
    for o in list(bpy.data.objects):
        if o is not obj:
            bpy.data.objects.remove(o, do_unlink=True)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    mesh = obj.data
    co = np.empty(len(mesh.vertices) * 3)
    mesh.vertices.foreach_get('co', co)
    co = co.reshape(-1, 3)
    lo, hi = co.min(0), co.max(0)
    k = p['length'] / float((hi - lo).max())
    co = (co - (lo + hi) / 2) * k
    mesh.vertices.foreach_set('co', co.ravel())
    mesh.update()

    uvs = mesh.uv_layers.active.data
    for old in list(mesh.color_attributes):
        mesh.color_attributes.remove(old)
    col = mesh.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    cache = {}
    for poly in mesh.polygons:
        loops = list(poly.loop_indices)
        corners = np.array([uvs[i].uv[:] for i in loops])
        centre = corners.mean(axis=0)
        pts = [centre] + [(centre + c) / 2 for c in corners]
        c = np.median(np.array([sample(q) for q in pts]), axis=0)
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
    bpy.ops.export_scene.gltf(
        filepath=p['out'],
        export_format='GLB',
        export_animations=False,
        export_materials='NONE',
        export_vertex_color='ACTIVE',
        export_texcoords=False,
        export_tangents=False,
        export_skins=False,
        export_morph=False,
        export_yup=True,
        export_image_format='NONE',
        export_active_vertex_color_when_no_material=True,
    )
    print('PROP', p['out'], len(mesh.vertices), 'verts', len(meshes), 'objects',
          [round(float(v), 3) for v in (hi - lo) * k], 'colours', len(set(map(tuple, cache.values()))))
