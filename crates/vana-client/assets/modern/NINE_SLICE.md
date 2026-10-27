# Nine-slice rendering contract

Each window master is 80x80 logical pixels; insets: 24 all around. In 2x atlas: 48px insets. Corner regions remain unscaled at UI scale 1. Top and bottom center strips repeat horizontally. Left and right strips repeat vertically. Center repeats in both axes. Minimum window size is 48x48 logical px. At noninteger UI scales, rasterize the SVG or use bilinear-filtered sprites with edge extrusion to avoid seams. Avoid premultiplied-alpha mismatch. `window_normal_border` has a transparent center for overlays.
