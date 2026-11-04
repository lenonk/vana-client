## Shadow blobs: kage_tex sampled at level 0 only (2026-11-03)

- Normal shadows showed dark patches on the ground around NPCs and other PCs that moved as the camera moved. High
  and Off did not: only Normal draws projected blobs (mode 0, see "Actor ground shadows").
- Cause: the blob is laid over every face of the 3 x 3 collision cells (12 x 12 units) with uv = (W.xz - pos.xz) /
  size_b + 0.5, mostly far outside 0..1, and CLAMP gives those faces kage_tex's edge texel. Level 0's edge is 0
  (the cone ends at radius ~124 of 128), but our upload gives the texture a full mip chain (retail's has none), and
  the small levels' edges are not: for a linear cone the edge texel is 0.008 at level 3 (32 x 32), 0.09 at level 5,
  0.18 at level 6 and 0.25 at levels 7 and 8. Distant and grazing faces pick those levels, so the whole cell block
  darkened by up to 2 x c x 0.25 (about 18% per blob, multiplying where blobs overlap), changing with view distance
  and angle.
- The kage sampler now has lod_max_clamp 0 (POINT at level 0, as retail). The silhouette layers (no mips) and the
  quad (uv inside 0..1) were not affected.
