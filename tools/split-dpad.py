# Cut the D-pad into its four arms (up, down, left, right) along its diagonals, so the 3D view
# can light each arm up on its own. Run by tools/convert-model.mjs; standalone:
#
#   blender -b --factory-startup --python tools/split-dpad.py -- <dpad.glb> <arms.glb>
#
# Input: a GLB with a single D-pad mesh, in the app's model frame (+x right, +y out of the face,
# +z towards the player). Output: a GLB with meshes "dpad_up", "dpad_down", "dpad_left" and
# "dpad_right", in the same frame and units, without materials (the caller assigns them).
# The cut edges of neighbouring arms coincide, so together they look exactly like the original.

import sys

import bmesh
import bpy
from mathutils import Vector

src, dst = sys.argv[sys.argv.index("--") + 1 :][:2]

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
meshes = [o for o in bpy.data.objects if o.type == "MESH"]
if len(meshes) != 1:
    raise SystemExit(f"expected one mesh in {src}, found {len(meshes)}")
dpad = meshes[0]

# Blender is Z-up: the importer maps glTF (x, y, z) to (x, -z, y), so the "up" arm (glTF -z,
# away from the player) points to Blender +y. Cut through the centre of the cross's footprint.
corners = [dpad.matrix_world @ Vector(c) for c in dpad.bound_box]
centre = sum(corners, Vector()) / 8
diag_a = Vector((1, 1, 0))  # positive side: up or right
diag_b = Vector((-1, 1, 0))  # positive side: up or left
ARMS = {"up": (1, 1), "right": (1, -1), "down": (-1, -1), "left": (-1, 1)}

bpy.ops.object.select_all(action="DESELECT")
for name, signs in ARMS.items():
    arm = dpad.copy()
    arm.data = dpad.data.copy()
    arm.name = arm.data.name = f"dpad_{name}"
    bpy.context.scene.collection.objects.link(arm)

    bm = bmesh.new()
    bm.from_mesh(arm.data)
    local_centre = arm.matrix_world.inverted() @ centre
    for normal, sign in zip((diag_a, diag_b), signs):
        n = (arm.matrix_world.inverted().to_3x3() @ normal).normalized() * sign
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        # clear_inner drops everything on the negative side of the plane, keeping the arm's half.
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=local_centre, plane_no=n, clear_inner=True)
    bm.to_mesh(arm.data)
    bm.free()
    arm.select_set(True)
    print(f"dpad_{name}: {len(arm.data.polygons)} faces")

bpy.data.objects.remove(dpad)
bpy.ops.export_scene.gltf(
    filepath=dst,
    export_format="GLB",
    use_selection=True,
    export_materials="NONE",
    export_yup=True,
    export_apply=False,
    export_animations=False,
)
