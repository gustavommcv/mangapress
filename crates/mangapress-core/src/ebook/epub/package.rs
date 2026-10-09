use super::{xml_escape, EpubOptions};

pub(super) fn build_container_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles>
<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles>
</container>"#
        .to_string()
}

pub(super) fn build_opf(
    options: &EpubOptions,
    identifier: &str,
    manifest_items: &[String],
    spine_itemrefs: &[String],
) -> String {
    let creators: String = options
        .authors
        .iter()
        .map(|author| format!("<dc:creator>{}</dc:creator>\n", xml_escape(author)))
        .collect();
    // Upstream's Kindle-only block: what tells Kindle's own renderer this is
    // a fixed-layout comic at the device's resolution, read in this
    // direction, with no gutter or margin of its own.
    let kindle_metas = match options.kindle_resolution {
        Some((width, height)) => format!(
            // `r##`: the border color's `"#` would end an `r#` string.
            r##"<meta name="fixed-layout" content="true"/>
<meta name="original-resolution" content="{width}x{height}"/>
<meta name="book-type" content="comic"/>
<meta name="primary-writing-mode" content="{writing_mode}"/>
<meta name="zero-gutter" content="true"/>
<meta name="zero-margin" content="true"/>
<meta name="ke-border-color" content="#FFFFFF"/>
<meta name="ke-border-width" content="0"/>
<meta name="orientation-lock" content="none"/>
<meta name="region-mag" content="true"/>
"##,
            writing_mode = if options.reading_direction.right_to_left != options.invert_direction {
                "horizontal-rl"
            } else {
                "horizontal-lr"
            },
        ),
        None => String::new(),
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="{lang}">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="pub-id">{identifier}</dc:identifier>
<dc:title>{title}</dc:title>
{creators}<dc:language>{lang}</dc:language>
<dc:contributor id="contributor">mangapress-{version}</dc:contributor>
{description}{series}<meta property="dcterms:modified">{modified}</meta>
<meta name="cover" content="{cover_id}"/>
{kindle_metas}<meta property="rendition:spread">landscape</meta>
<meta property="rendition:layout">pre-paginated</meta>
</metadata>
<manifest>
<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
{items}
</manifest>
<spine toc="ncx" page-progression-direction="{direction}">
{spine}
</spine>
</package>"#,
        lang = xml_escape(&options.language),
        identifier = xml_escape(identifier),
        title = xml_escape(&options.title),
        version = env!("CARGO_PKG_VERSION"),
        cover_id = if options.cover.is_some() {
            "cover"
        } else {
            "img1"
        },
        series = match (&options.series, options.kindle) {
            (Some((name, position)), false) => format!(
                // `r##`: `"#c02"` would end an `r#` string.
                r##"<meta property="belongs-to-collection" id="c02">{name}</meta>
<meta refines="#c02" property="collection-type">series</meta>
{position}"##,
                name = xml_escape(name),
                position = position
                    .as_deref()
                    .map(|p| format!(
                        "<meta refines=\"#c02\" property=\"group-position\">{}</meta>\n",
                        xml_escape(p)
                    ))
                    .unwrap_or_default(),
            ),
            _ => String::new(),
        },
        modified = xml_escape(&options.modified),
        description = options
            .description
            .as_deref()
            .map(|d| format!("<dc:description>{}</dc:description>\n", xml_escape(d)))
            .unwrap_or_default(),
        items = manifest_items.join("\n"),
        direction = if options.reading_direction.right_to_left != options.invert_direction {
            "rtl"
        } else {
            "ltr"
        },
        spine = spine_itemrefs.join("\n"),
    )
}
