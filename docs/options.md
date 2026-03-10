subrandr supports various customization options that change how subtitles are rendered or provide functionality helpful for debugging.

In the C API, options can be set using the interface exposed by the `subrandr/config.h` header.  
For information on setting them in a particular program that uses subrandr, consult its documentation. Individual programs can provide their own interface that wraps subrandr's options, in which case you might need to read the documentation for that interface instead of this document.   
Defaults can be overridden process-wide by setting the `SBR_CONFIG` environment variable but this is intended as a debugging escape hatch and may be removed/changed in API-compatible releases.

Value grammar is defined using [CSS Value Definition Syntax](https://www.w3.org/TR/css-values-3/#value-defs) with the following caveats:
- Bracketed range notation is also used below to specify non-range restrictions.
- The values are *not* parsed like real CSS Values, but the intention is that this will be changed in the future.
- 3 and 4 character hex colors are not supported yet.

## API stability

> [!NOTE]
> The stability guarantees of these options are purposefully vague because it is not clear how volatile they will really be in practice. Maybe I won't change them for years in which case the guarantees can be strengthened.

Changes to options not explicitly specified as unstable have best-effort stablility guarantees across API-compatible versions.
They may still be renamed, removed, or have their syntax changed but new versions will still try to accept old names/syntax (unless an option is removed with no replacement).
Such translation functionality may or may not be removed after a long enough time.

## SRV3-specific options

### srv3-layout-mode

<table>
  <tr><td><i>Default</td> <td>inline-block</td></tr>
  <tr><td><i>Grammar</td> <td>inline-block | inline</td></tr>
</table>

Specifies the layout mode to use for SRV3 segments.

Can be either of:
- `inline-block`: Corresponds to what YouTube's web player does. Wraps segment runs sharing a single pen in an `inline-block`. Background is applied to the `inline-block` instead of individual spans.
- `inline`: An alternative layout mode that does not wrap segment runs in `inline-block`s, putting each one in an `inline` instead. `inline-sizing` is set to `stretch` to better match `inline-block` behavior.
  
  This mode avoids breaking up text shaping across segment style changes which stops segments from jumping around in karaoke subtitles. However there are cases where it "breaks" due to sizing differences or subtitles (accidentally) relying on very specific `inline-block` interactions.

### srv3-default-font-size

<table>
  <tr><td><i>Default</td> <td>100</td></tr>
  <tr><td><i>Grammar</td> <td>&lt;integer [0,2^16-1]&gt;</td></tr>
</table>

Overrides the default font size (`sz` attribute) of SRV3 pens.

### srv3-default-font-style

<table>
  <!-- it's actually zero as of 2026-03-08 but that's effectively 4 -->
  <tr><td><i>Default</td> <td>4</td></tr>
  <tr><td><i>Grammar</td> <td>&lt;integer [1,7]&gt;</td></tr>
</table>

Overrides the default font style (`fs` attribute) of SRV3 pens.

### srv3-default-fg-color

<table>
  <tr><td><i>Default</td> <td><code>#FFFFFFFF</code></td></tr>
  <tr><td><i>Grammar</td> <td>&lt;hex-color&gt;</td></tr>
</table>


Overrides the default foreground color (`fc` and `fo` attributes) of SRV3 pens.  

### srv3-default-bg-color

<table>
  <tr><td><i>Default</td> <td><code>#080808BF</code></td></tr>
  <tr><td><i>Grammar</td> <td>&lt;hex-color&gt;</td></tr>
</table>

Overrides the default background color (`bc` and `bo` attributes) of SRV3 pens.  

### srv3-default-edge-type

<table>
  <tr><td><i>Default</td> <td>none</td></tr>
  <tr><td><i>Grammar</td> <td>none | hard-shadow | bevel | glow | soft-shadow</td></tr>
</table>

Overrides the default edge type (`et` attribute) of SRV3 pens.

Must be one of:
- `none`: No edge decorations.
- `hard-shadow`: Primary color shadow.
- `soft-shadow`: Primary color shadow with soft edges.
- `bevel`: One primary color shadow offset to the top left and one secondary color shadow offset to the bottom right
- `glow`: Soft primary color outline.

If the edge color is defined then the primary color and secondary color are equal to the edge color.  
Otherwise the primary color is `#222222` with alpha copied from the foreground color and the secondary color is `#CCCCCC` with alpha copied from the foreground color.

### srv3-default-edge-color

<table>
  <tr><td><i>Default</td> <td>none</td></tr>
  <tr><td><i>Grammar</td> <td>&lt;hex-color [without alpha]&gt; | none</td></tr>
</table>

Overrides the default edge color (`ec` attribute) of SRV3 pens.  

If `none`, the edge color is not defined and edge decorations will use the fallback color calculations described in `srv3-edge-type` above.

## Testing/debugging options

Options meant to be used while working on subrandr or diagnosing issues.

> [!WARNING]
> These are unstable and their shape or existence should not be relied upon.

### debug-dpi-override

<table>
  <tr><td><i>Default</td> <td>none</td></tr>
  <tr><td><i>Grammar</td> <td>&lt;integer&gt; | none</td></tr>
</table>

If not `none`, overrides the DPI used by the renderer, ignoring the value provided via the C API.

### debug-draw-version-overlay

<table>
  <tr><td><i>Default</td> <td>no</td></tr>
  <tr><td><i>Grammar</td> <td>yes | no</td></tr>
</table>

If enabled, an overlay with version and rasterizer information will be drawn in the top-left corner.

### debug-draw-perf-overlay

<table>
  <tr><td><i>Default</td> <td>no</td></tr>
  <tr><td><i>Grammar</td> <td>yes | no</td></tr>
</table>

If enabled, an overlay with frame times and an accompanying graph will be drawn in the top-right corner.
