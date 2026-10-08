"""Offline curl substitute; only the download boundary is mocked."""

import json
import os
from pathlib import Path
import shutil
import sys


catalog = json.loads(Path(os.environ["INSTALLER_CATALOG"]).read_text(encoding="utf-8"))
arguments = sys.argv[1:]
url = next(argument for argument in arguments if argument.startswith("https://"))
with Path(catalog["log"]).open("a", encoding="utf-8") as log:
    log.write(url + "\n")
if url in catalog["fail_urls"]:
    sys.exit("Fixture download failed")
if url == catalog["latest_web"]:
    print(catalog["latest_redirect"], end="")
else:
    source = catalog["files"].get(url)
    if source is None:
        sys.exit(f"Unexpected fixture URL: {url}")
    destination = arguments[arguments.index("-o") + 1]
    shutil.copyfile(source, destination)
