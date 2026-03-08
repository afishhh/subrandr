subrandr supports various customization options that change how subtitles are rendered or provide functionality helpful for debugging.

Options are exposed in the C API in the `subrandr/config.h` header. For information on setting them in a particular host program, consult its documentation. Individual programs can provide their own interface that wraps subrandr's options in which case you might not need to touch subrandr's options at all. subrandr defaults can be overridden process-wide by setting the `SBR_CONFIG` environment variable but this is intended as a debugging escape hatch and may be removed/changed in API-compatible releases.

## SRV3-specific options

### srv3-layout-mode

<table>
  <tr><th>Default</th> <td>inline-block</td></tr>
</table>

Specifies the layout mode to use for SRV3 segments.  
Can be either of:
- `inline-block`: Corresponds to what YouTube's web player does. Wraps segment runs sharing a single pen in an `inline-block`. Background is applied to the `inline-block` instead of individual spans.
- `inline`: An alternative layout mode that does not wrap segments in `inline-block`s, keeping them as `inline`s. `inline-sizing` is set to `stretch` to better match SRV3 background sizing.
  
  This mode avoids breaking up text shaping across segment style changes which makes segments jump around less. However there are cases where it "breaks" due to sizing differences or subtitles (accidentally) relying on very specific `inline-block` interactions.

### srv3-default-font-size

<table>
  <tr><th>Default</th> <td>100</td></tr>
</table>

Overrides the default font size of SRV3 pens.

### srv3-default-font-style

<!-- it's actually zero as of 2026-03-08 but that's effectively 4 -->
<table>
  <tr><th>Default</th> <td>4</td></tr>
</table>

Overrides the default font style of SRV3 pens.

### srv3-default-fg-color

<table>
  <tr><th>Default</th> <td><code>#FFFFFFFF</code></td></tr>
</table>


Overrides the default foreground color of SRV3 pens.  
Must be a `#RRGGBB(AA)` hex color code (with optional alpha).

### srv3-default-bg-color

<table>
  <tr><th>Default</th> <td><code>#080808BF</code></td></tr>
</table>

Overrides the default background color of SRV3 pens.  
Must be a `#RRGGBB(AA)` hex color code (with optional alpha).

### srv3-edge-type

<table>
  <tr><th>Default</th> <td>none</td></tr>
</table>

Overrides the default edge type of SRV3 pens.

Must be one of:
- `none`: No edge decorations.
- `hard-shadow`: Opaque text shadow with primary color.
- `soft-shadow`: TODO
- `bevel`: One text shadow with secondary color offset to the bottom right and another with primary color offset to the top left.
- `glow`: TODO

If the edge color is defined then the primary color and secondary color are equal to the edge color.  
Otherwise the primary color is `#222222` with alpha copied from the foreground color and the secondary color is `#CCCCCC` with alpha copied from the foreground color.

### srv3-edge-color

<table>
  <tr><th>Default</th> <td>none</td></tr>
</table>

Overrides the default edge color of SRV3 pens.  
Must be either `none` or a `#RRGGBB` hex color code.

A value of `none` means the edge color is not defined and edge decorations will use the fallback color calculations described above.

## debug-*

Debugging options meant to be used while working on subrandr or diagnosing issues.

> [!WARNING]
> These are extra unstable and their shape or existence should not be relied upon in any way.

### debug-dpi-override

<table>
  <tr><th>Default</th> <td>none</td></tr>
</table>

Overrides the DPI used by the renderer, ignoring the value provided via the C API.

### debug-draw-version-overlay

<table>
  <tr><th>Default</th> <td>no</td></tr>
</table>

If enabled, an overlay with version information as well as some other information will be drawn in the top-left corner.

### debug-draw-perf-overlay

<table>
  <tr><th>Default</th> <td>no</td></tr>
</table>

If enabled, an overlay with frame times and an accompanying graph will be drawn in the top-right corner.
