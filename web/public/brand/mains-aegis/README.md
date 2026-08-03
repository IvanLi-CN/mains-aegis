# Mains Aegis Brand Assets

## Authority

- Geometry source: `output/imagegen/mains-aegis-logo-round-4/06-continuous-ribbon-low-wide.png`.
- Fixed source crop: `[141, 361, 884, 661]`, represented by SVG `viewBox="0 0 743 300"`.
- Palette source: `output/imagegen/mains-aegis-logo-06-light-color-model-grid-r2/mains-aegis-logo-06-light-color-model-grid-r2.png`.

The color grid is not a geometry source. Its nine model-generated marks have small contour differences, so using it as a shared path source would create an unstable logo system.

## Construction

The mark is not an infinity glyph, a letterform, or a mirrored emblem. It is read as two continuous power ribbons passing through an asymmetric handoff:

1. `left-main` is the incoming ribbon and central handoff.
2. `left-tail` is the deliberately detached lower termination wedge.
3. `right-ribbon` is the outgoing ribbon and right terminal.

`mains-aegis-logo-mark.svg` contains exactly those three named closed paths. It is an ordinary SVG vector asset, with no embedded raster, filter, mask, pattern, or gradient. The curves use a small tangent-continuous cubic Bezier construction; the deliberate cut edges remain straight. The colored derivatives use byte-identical `d` data and change only flat fills.

## Assets

- `mains-aegis-logo-mark.svg`: transparent monochrome mark master using `currentColor`.
- `mains-aegis-logo-mark-color-light.svg`: colored mark for light surfaces.
- `mains-aegis-logo-mark-color-dark.svg`: colored mark for dark surfaces.
- `mains-aegis-logo-square.svg`: approved transparent `1024 x 1024` monochrome master; set its color through CSS `color`/`currentColor`.
- `mains-aegis-logo-square-color-light.svg`: colored square logo for light surfaces.
- `mains-aegis-logo-square-color-dark.svg`: colored square logo for dark surfaces.
- `mains-aegis-logo-wide.svg`: transparent horizontal Banner Logo master using CSS `color`/`currentColor`.
- `mains-aegis-logo-wide-color-light.svg`: colored horizontal Banner Logo for light surfaces.
- `mains-aegis-logo-wide-color-dark.svg`: colored horizontal Banner Logo for dark surfaces.
- `mains-aegis-logo-manifest.json`: source, construction, palette, and geometry hash.

## Selected Square Lockup

The selected direction is `09`: rounded instrument-panel capitals with stable horizontal and vertical structure, controlled corners, and a normal horizontal reading order. Its raster cell is the wordmark geometry reference. The delivery asset contains a normalized closed SVG path derived from that silhouette; it does not embed the raster image or depend on a font.

It is a standard vertical brand lockup, not an App icon. The transparent `1024 x 1024` canvas holds the unchanged `06` mark horizontally at `x=152, y=252, w=720`, with one intact `MAINS AEGIS` wordmark at `x=192, y=704, w=640, h=69`. Those dimensions reproduce the selected review composition instead of stretching its wordmark to fill the former `96px` review box. The mark remains the dominant first-read element. Its three paths retain their original IDs, order, and exact `d` data from the horizontal master; their group only applies the fixed translation and uniform scale.

The approved asset contains only the SVG root, title, `mark-06` group, the three mark paths, and one `wordmark-mains-aegis` compound path using `currentColor`. It contains no background, `rect`, `text`, image, font dependency, filter, mask, pattern, gradient, `defs`, rotation, or stroke. Previous review candidates are not retained in the delivery asset set.

The existing no-text PWA icon remains intentionally separate.

## Rebuild And Verify

```sh
python3 tools/brand/reconstruct_mains_aegis_logo.py
python3 tools/brand/reconstruct_mains_aegis_logo.py --strict
```

The first command writes SVG assets and evidence under `output/logo-vector-validation/` and `output/logo-square-lockup-validation/`. The strict command checks vector integrity, path topology, cubic-curve continuity, finite node count, flat-color accuracy, light-theme contrast, fixed-origin reference comparison, the approved square-lockup path identity, the selected 09 wordmark overlay, banned SVG features, fixed layout geometry, and uncropped `1024px`, `512px`, `256px`, `128px`, and `64px` renders on white, light-Web, and dark-Web surfaces.

The source is a bitmap, so its antialiasing is not an exact path specification. The reference comparison intentionally permits normal renderer and one-pixel edge differences while preventing a material drift in the construction. It is never auto-aligned, and the report does not claim raw pixel-for-pixel identity.
