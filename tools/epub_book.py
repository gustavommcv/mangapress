"""Read generated EPUB semantics for parity and complete-book regression checks."""

import posixpath
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET
import zipfile

NS = {"opf": "http://www.idpf.org/2007/opf", "dc": "http://purl.org/dc/elements/1.1/",
      "x": "http://www.w3.org/1999/xhtml", "ncx": "http://www.daisy.org/z3986/2005/ncx/",
      "ocf": "urn:oasis:names:tc:opendocument:xmlns:container"}
OPS = "{http://www.idpf.org/2007/ops}type"


def member(base, reference):
    """Resolve a local EPUB URI, rejecting external or escaping references."""
    uri = urlsplit(reference)
    if uri.scheme or uri.netloc:
        raise ValueError(f"external book reference: {reference}")
    path = posixpath.normpath(posixpath.join(posixpath.dirname(base), unquote(uri.path)))
    if path.startswith(("../", "/")) or path == "..":
        raise ValueError(f"book reference escapes archive: {reference}")
    return path


def navigation_tree(document, kind, base, hrefs):
    """Keep both the hierarchy and each link's actual zero-based spine target."""
    def children(parent):
        entries = parent.findall("ncx:navPoint" if kind == "ncx" else "x:li", NS)
        nodes = []
        for entry in entries:
            if kind == "ncx":
                label = entry.find("ncx:navLabel/ncx:text", NS).text
                reference = entry.find("ncx:content", NS).attrib["src"]
                nested = children(entry)
            else:
                link = entry.find("x:a", NS)
                label, reference = link.text, link.attrib["href"]
                nested = children(entry.find("x:ol", NS)) if entry.find("x:ol", NS) is not None else ()
            nodes.append((label, hrefs.index(member(base, reference)), nested))
        return tuple(nodes)

    if kind == "ncx":
        parent = document.find("ncx:navMap", NS)
    else:
        toc = next(node for node in document.findall(".//x:nav", NS) if node.attrib.get(OPS) == "toc")
        parent = toc.find("x:ol", NS)
    tree = children(parent)
    if not tree:
        raise ValueError(f"empty {kind} navigation")
    return tree


def flatten_navigation(tree):
    entries = []
    for label, index, nested in tree:
        entries.append((label, index))
        entries.extend(flatten_navigation(nested))
    return tuple(entries)


def read_epub(path):
    with zipfile.ZipFile(path) as archive:
        if archive.read("mimetype") != b"application/epub+zip":
            raise ValueError("invalid EPUB mimetype")
        container = ET.fromstring(archive.read("META-INF/container.xml"))
        opf_path = container.find(".//ocf:rootfile", NS).attrib["full-path"]
        opf = ET.fromstring(archive.read(opf_path))
        items = {item.attrib["id"]: item for item in opf.findall("opf:manifest/opf:item", NS)}
        # Verify every declared resource, even if it is not in the spine.
        for item in items.values():
            archive.getinfo(member(opf_path, item.attrib["href"]))
        metadata = {key: tuple(node.text or "" for node in opf.findall(f"opf:metadata/dc:{key}", NS))
                    for key in ("title", "creator", "language", "description")}
        metadata["creators"] = tuple(sorted(metadata.pop("creator")))
        layout_keys = {"primary-writing-mode", "rendition:layout", "rendition:spread"}
        metadata["layout"] = tuple(sorted((key, node.attrib.get("content", node.text or ""))
                                          for node in opf.findall("opf:metadata/opf:meta", NS)
                                          if (key := node.attrib.get("name", node.attrib.get("property"))) in layout_keys))
        spine = opf.find("opf:spine", NS)
        direction = spine.attrib.get("page-progression-direction")
        pages, hrefs, sides, documents = [], [], [], []
        for ref in spine:
            item = items[ref.attrib["idref"]]
            href = member(opf_path, item.attrib["href"])
            document = ET.fromstring(archive.read(href))
            # Kindle markup can repeat the same image in its hidden block.
            images = {member(href, node.attrib["src"]) for node in document.findall(".//x:img", NS)}
            if len(images) != 1:
                raise ValueError(f"expected one page image in {href}, got {len(images)}")
            pages.append(archive.read(images.pop()))
            hrefs.append(href)
            documents.append(document)
            properties = ref.attrib.get("properties", "").split()
            sides.append(tuple(sorted(value.replace("rendition:", "") for value in properties)))
        if not pages:
            raise ValueError("empty EPUB spine")
        covers = [item for item in items.values() if "cover-image" in item.attrib.get("properties", "").split()]
        if len(covers) != 1:
            raise ValueError(f"expected one cover, got {len(covers)}")
        cover = archive.read(member(opf_path, covers[0].attrib["href"]))
        navigation, trees = {}, {}
        for kind in ("ncx", "nav"):
            item = next(item for item in items.values() if
                        (item.attrib["media-type"] == "application/x-dtbncx+xml" if kind == "ncx"
                         else "nav" in item.attrib.get("properties", "").split()))
            href = member(opf_path, item.attrib["href"])
            document = ET.fromstring(archive.read(href))
            trees[kind] = navigation_tree(document, kind, href, hrefs)
            navigation[kind] = flatten_navigation(trees[kind])
        if trees["ncx"] != trees["nav"]:
            raise ValueError("NCX and EPUB3 navigation disagree")
        # Existing flat parity fields are preserved. Extra inspection fields are
        # for explicit book assertions, not serialization/UUID/timestamp parity.
        return {"metadata": metadata, "direction": direction, "sides": sides,
                "navigation": navigation, "pages": pages, "cover": cover,
                "toc": trees["nav"], "package": opf, "documents": documents}
