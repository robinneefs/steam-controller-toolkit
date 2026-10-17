# Steam Controller 3D model

`steam-controller.glb` is adapted from
**["Steam Machine + Controller"](https://sketchfab.com/3d-models/steam-machine-controller-c1fcf77d71984c11818a5e2412816705)
by [dark_igorek](https://sketchfab.com/dark_igorek)**, licensed under
[Creative Commons Attribution 4.0 International (CC BY 4.0)](https://creativecommons.org/licenses/by/4.0/).

**Changes made** (`tools/convert-model.mjs`): only the controller was kept (the Steam Machine
was removed), the model was scaled to centimetres and centred, its buttons, sticks, trackpads,
triggers and back buttons were split into separate parts so they can move, unused vertex data
was removed, textures were re-encoded as WebP, and the geometry was quantized and compressed.

This adapted model is also shared under CC BY 4.0; see `LICENSE.txt`. dark_igorek does not
endorse this project.
