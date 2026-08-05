#!/usr/bin/env python3
"""Upload ROM files to RomM using its chunked upload endpoints.

RomM does not take a whole file in one request. The sequence is:

    POST /api/roms/upload/start      headers describe platform, name, size, chunks
    PUT  /api/roms/upload/{id}       one call per chunk, index in a header
    POST /api/roms/upload/{id}/complete

`x-upload-platform` is the platform's **numeric id**, not its slug, so the
platform list is fetched and matched first.

A failed upload is cancelled rather than left half-written, since an abandoned
upload id would otherwise sit on the server holding a partial file.
"""

import argparse
import base64
import json
import pathlib
import sys
import urllib.error
import urllib.request

CHUNK = 8 * 1024 * 1024


class Api:
    def __init__(self, url, user, password):
        self.url = url.rstrip("/")
        self.auth = base64.b64encode(f"{user}:{password}".encode()).decode()

    def _req(self, method, path, *, data=None, headers=None):
        h = {"Authorization": f"Basic {self.auth}"}
        h.update(headers or {})
        req = urllib.request.Request(f"{self.url}{path}", method=method, data=data, headers=h)
        with urllib.request.urlopen(req, timeout=300) as r:
            raw = r.read()
            return json.loads(raw) if raw else None

    def platforms(self):
        return self._req("GET", "/api/platforms")

    def start(self, platform_id, filename, size, chunks):
        return self._req("POST", "/api/roms/upload/start", headers={
            "x-upload-platform": str(platform_id),
            "x-upload-filename": filename,
            "x-upload-total-size": str(size),
            "x-upload-total-chunks": str(chunks),
        })

    def chunk(self, upload_id, index, blob):
        return self._req("PUT", f"/api/roms/upload/{upload_id}", data=blob, headers={
            "x-chunk-index": str(index),
            "Content-Type": "application/octet-stream",
        })

    def complete(self, upload_id):
        return self._req("POST", f"/api/roms/upload/{upload_id}/complete")

    def cancel(self, upload_id):
        try:
            self._req("POST", f"/api/roms/upload/{upload_id}/cancel")
        except Exception:
            pass


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--url", default="http://dev.lan")
    ap.add_argument("--user", required=True)
    ap.add_argument("--password", required=True)
    ap.add_argument("--platform", required=True, help="platform slug, e.g. neogeoaes")
    ap.add_argument("--apply", action="store_true")
    args = ap.parse_args()

    api = Api(args.url, args.user, args.password)
    plats = {p["fs_slug"]: p for p in api.platforms()}
    target = plats.get(args.platform)
    if not target:
        sys.exit(f"no platform {args.platform!r}; have: {', '.join(sorted(plats))}")
    print(f"platform {args.platform} -> id {target['id']} ({target.get('rom_count')} roms)")

    for f in args.files:
        p = pathlib.Path(f)
        if not p.is_file():
            print(f"  skip {p}: not a file")
            continue
        size = p.stat().st_size
        chunks = max(1, -(-size // CHUNK))
        print(f"  {p.name}  {size/1e6:.1f} MB in {chunks} chunk(s)")
        if not args.apply:
            continue

        started = api.start(target["id"], p.name, size, chunks)
        upload_id = started.get("upload_id") or started.get("id")
        if not upload_id:
            print(f"    FAILED: no upload id in {started}")
            continue
        try:
            with p.open("rb") as fh:
                for i in range(chunks):
                    api.chunk(upload_id, i, fh.read(CHUNK))
                    print(f"    chunk {i+1}/{chunks}")
            api.complete(upload_id)
            print("    complete")
        except urllib.error.HTTPError as e:
            api.cancel(upload_id)
            print(f"    FAILED {e.code}: {e.read()[:200]}")
        except Exception as e:
            api.cancel(upload_id)
            print(f"    FAILED: {e}")

    if not args.apply:
        print("\n(dry run — pass --apply)")


if __name__ == "__main__":
    main()
