"""Package weapon variants, item pictures and floor sprites from authored PNGs.

CLI: py compile_weapons.py artwork/build.json artwork/legendary_weapons.json --output target/weapons
Manifest: {"weapons": [{"name": "kusanagi", "resource": "samurai_kusanagi",
"png": "source/kusanagi.png", "rect": [0,0,1728,240], "downsample": 24,
"source_grip": [14,5]}]}. PNG paths are relative to the manifest. An optional
"illustration" RGBA PNG becomes the item description (collection) picture as drawn.
Only generated staging configs contain absolute paths. Runtime installation is separate.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import shutil

from PIL import Image

from compile import compile_package, raw_image, sampled_weapon_png
from ro_formats import validate_pair, write_act, write_spr


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8", newline="\n")


def item_picture(source, size):
    """Rotate the authored crop, fit without stretching, and retain its RGBA."""
    image = source.rotate(45, Image.Resampling.NEAREST, expand=True)
    ratio = (size - 4) / max(image.size)
    image = image.resize(tuple(max(1, round(v * ratio)) for v in image.size), Image.Resampling.NEAREST)
    canvas = Image.new("RGBA", (size, size))
    canvas.alpha_composite(image, ((size - image.width) // 2, (size - image.height) // 2))
    return canvas


def illustration_picture(path, size):
    """Fit an authored RGBA illustration into a square without rotating or stretching it."""
    image = Image.open(path).convert("RGBA")
    ratio = (size - 4) / max(image.size)
    resized = image.convert("RGBa").resize(tuple(max(1, round(v * ratio)) for v in image.size), Image.Resampling.LANCZOS).convert("RGBA")
    canvas = Image.new("RGBA", (size, size))
    canvas.alpha_composite(resized, ((size - resized.width) // 2, (size - resized.height) // 2))
    return canvas


def save_colorkey_bmp(path, rgba):
    """RO BMPs use binary magenta transparency; no magenta-blended edge pixels."""
    rgb = Image.new("RGB", rgba.size, (255, 0, 255))
    mask = rgba.getchannel("A").point(lambda alpha: 255 if alpha >= 128 else 0)
    rgb.paste(rgba.convert("RGB"), (0, 0), mask)
    path.parent.mkdir(parents=True, exist_ok=True)
    rgb.save(path, format="BMP")


def compile_weapons(base_path, manifest_path, output):
    base_path, manifest_path, output = (Path(p).resolve() for p in (base_path, manifest_path, output))
    base = json.loads(base_path.read_text(encoding="utf-8"))
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    entries = manifest["weapons"]
    if not isinstance(entries, list) or not entries:
        raise ValueError("Manifest weapons must be a nonempty list")
    names, resources = set(), set()
    for entry in entries:
        name, resource = entry["name"], entry["resource"]
        if any(not isinstance(v, str) or not re.fullmatch(r"[a-z][a-z0-9_]*", v) for v in (name, resource)):
            raise ValueError("Weapon names and resources must be plain lowercase ASCII basenames")
        if name == base.get("body", "ember_ronin") or name in names or resource in resources:
            raise ValueError("Weapon names/resources must be unique and not replace the body")
        names.add(name)
        resources.add(resource)
    overlay = output / "overlay"
    armed = overlay / "data/sprite/custom/samurai"
    armed.mkdir(parents=True, exist_ok=True)
    report = {"base_config_sha256": digest(base_path), "manifest_sha256": digest(manifest_path), "weapons": {}}
    body_bytes = None
    for entry in entries:
        name, resource = entry["name"], entry["resource"]
        config = copy.deepcopy(base)
        for sheet in config["sheets"].values():
            sheet["path"] = str((base_path.parent / sheet["path"]).resolve())
        png = (manifest_path.parent / entry["png"]).resolve()
        config["weapon"] = {key: entry[key] for key in ("rect", "downsample", "source_grip") if key in entry}
        config["weapon"]["png"] = str(png)
        config["weapon_name"] = name
        destination = output / name
        destination.mkdir(parents=True, exist_ok=True)
        config_path = destination / "build.json"
        save_json(config_path, config)
        compile_package(config_path, destination)
        body = tuple((destination / f'{config.get("body", "ember_ronin")}.{ext}').read_bytes() for ext in ("spr", "act"))
        if body_bytes is not None and body != body_bytes:
            raise ValueError(f"Weapon variant {name} changed the body SPR/ACT")
        body_bytes = body
        # Off-hand (dual wield) pairs exist when the base config has offhand_poses.
        pairs = [n for n in (name, f"{name}_offhand") if (destination / f"{n}.act").is_file()]
        for pair in pairs:
            for extension in ("spr", "act"):
                shutil.copy2(destination / f"{pair}.{extension}", armed / f"{pair}.{extension}")
        source = sampled_weapon_png(png, {"rect": entry["rect"]} if "rect" in entry else {})
        small = item_picture(source, 32)
        save_colorkey_bmp(overlay / f"data/texture/유저인터페이스/item/{resource}.bmp", small)
        # The item description picture: the authored illustration when there is one.
        illustration = entry.get("illustration")
        collection = illustration_picture(manifest_path.parent / illustration, 128) if illustration else item_picture(source, 128)
        save_colorkey_bmp(overlay / f"data/texture/유저인터페이스/collection/{resource}.bmp", collection)
        floor = overlay / f"data/sprite/아이템/{resource}"
        write_spr(floor.with_suffix(".spr"), [raw_image(small)])
        # A dropped item is centered on its world position: explicit origin (16,16).
        layer = dict(sprite_index=0, sprite_type=1, width=32, height=32, x=0, y=0)
        write_act(floor.with_suffix(".act"), [dict(interval=8, frames=[dict(layers=[layer], anchors=[])])])
        floor_report = validate_pair(floor.with_suffix(".spr"), floor.with_suffix(".act"))
        report["weapons"][name] = dict(resource=resource, source_sha256=digest(png),
            weapon_sha256={ext: digest(armed / f"{name}.{ext}") for ext in ("spr", "act")},
            offhand_sha256={ext: digest(armed / f"{name}_offhand.{ext}") for ext in ("spr", "act")} if len(pairs) > 1 else None,
            floor_valid=floor_report["valid"], floor_origin=[16, 16], icon_size=[32, 32], collection_size=[128, 128])
    report["body_sha256"] = {ext: hashlib.sha256(data).hexdigest() for ext, data in zip(("spr", "act"), body_bytes)}
    report["overlay_sha256"] = {p.relative_to(overlay).as_posix(): digest(p) for p in sorted(overlay.rglob("*")) if p.is_file()}
    save_json(output / "weapons_report.json", report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base_config")
    parser.add_argument("manifest")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    print(json.dumps(compile_weapons(args.base_config, args.manifest, args.output), indent=2))
