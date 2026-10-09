use super::xml_escape;

/// One page's XHTML. Upstream reference: `buildHTML()` in upstream's
/// `comic2ebook.py` — the image is an *inline* `<img>` carrying its own pixel
/// size as `width`/`height` attributes, inside a `text-align:center` block,
/// under the same two stylesheet rules upstream ships in its `style.css`.
///
/// The image is deliberately never sized in percentages, and never made
/// `display: block`. An earlier version of this function wrote `img {
/// display: block; width: 100%; height: 100%; }` instead, which is harmless
/// in a reader that honors the fixed-layout viewport (the viewport is the
/// image's own size, so 100% of it changes nothing) but not in KOReader:
/// crengine ignores that viewport, lays the page out against the screen, and
/// sizes a block image's two axes independently. Confirmed by rendering a
/// real volume through KOReader's own crengine at 1072x1448, not assumed: a
/// 965x1448 page came out 1072x1448, stretched 11% sideways (about 15% with
/// KOReader's default margins and status bar), where upstream's markup for
/// the same page kept its proportions. The two obvious half-measures were
/// rendered the same way and both still stretch — `width: 100%; height:
/// auto` by the same 11-15%, and a `display: block` image with fixed
/// `width`/`height` by 7% once margins leave it less than its own height —
/// so this follows upstream's markup as a whole rather than patching the one
/// property that first looked wrong.
///
/// `width`/`height` are `0` when the page's header couldn't be read (see the
/// caller): the viewport and the image's size attributes are then both left
/// out, and the image falls back to its intrinsic size.
///
/// Two more things upstream writes, both carried over: a dark page gets a
/// black background on its `<body>` (so the strip a centered page leaves
/// beside it is black like the page, not white), and a Kindle profile gets
/// a hidden, non-empty first block inside the centered one, which upstream
/// adds because Kindle's own renderer misplaces the page without it.
pub(super) fn build_page_xhtml(
    title: &str,
    image_path_from_oebps: &str,
    width: u32,
    height: u32,
    black_background: bool,
    kindle: bool,
) -> String {
    // Text/cNNNN/pNNNN.xhtml -> Images/cNNNN/pNNNN.ext is always two levels
    // up then back down, given the fixed directory layout above.
    let image_href = format!("../../{image_path_from_oebps}");
    let (viewport, image_size) = if width > 0 && height > 0 {
        (
            format!(r#"<meta name="viewport" content="width={width}, height={height}"/>"#),
            format!(r#" width="{width}" height="{height}""#),
        )
    } else {
        (String::new(), String::new())
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
<title>{title}</title>
<meta charset="utf-8"/>
{viewport}
<style type="text/css">
@page {{ margin: 0; }}
body {{ display: block; margin: 0; padding: 0; }}
</style>
</head>
<body{body_style}>
<div style="text-align:center;">
{kindle_block}<img{image_size} src="{image_href}" alt=""/>
</div>
</body>
</html>"#,
        title = xml_escape(title),
        body_style = if black_background {
            r#" style="background-color:#000000;""#
        } else {
            ""
        },
        kindle_block = if kindle {
            "<div style=\"display:none;\">.</div>\n"
        } else {
            ""
        },
    )
}
