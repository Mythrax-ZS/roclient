"""Strict, dependency-free Ragnarok SPR 2.1 and ACT 2.5 serialization.

This module packages already-created artwork; it does not draw or alter artwork.
All input pixels use top-down row order. Truecolor pixels use RGBA in this API,
and are stored as bottom-up ABGR in SPR files. Indexed pixels use one byte each,
top-down, with index zero transparent. The palette is 256 RGBA entries (1024
bytes), shared by every indexed image. ACT layer indices are relative to their
image type, not absolute indices in the combined SPR image list.

Primary format implementations consulted (2026-10-29):
https://github.com/Tokeiburu/GRFEditor/blob/master/GRF/FileFormats/ActFormat/ActConverter.cs
https://github.com/Tokeiburu/GRFEditor/blob/master/GRF/FileFormats/ActFormat/Layer.cs
https://github.com/Tokeiburu/GRFEditor/blob/master/GRF/FileFormats/ActFormat/Frame.cs
https://github.com/Tokeiburu/GRFEditor/blob/master/GRF/FileFormats/SprFormat/SprHeader.cs
https://github.com/MrAntares/roBrowserLegacy/blob/master/src/Loaders/Sprite.js
https://github.com/MrAntares/roBrowserLegacy/blob/master/src/Loaders/Action.js
https://github.com/MrAntares/roBrowserLegacy/blob/master/src/Renderer/Entity/EntityAction.js

ACT interval is stored in raw ticks. roBrowser multiplies it by 25 milliseconds;
client attack/walk timing can override it. No playback/loop flag exists in ACT.
Player action index is group * 8 + direction. Body anchors position separately
loaded head/equipment; they do not suppress the client's default head/weapon.

CLI: py ro_formats.py validate character.spr character.act --player
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
from typing import Any, Mapping, Sequence


DIRECTIONS = ("S", "SW", "W", "NW", "N", "NE", "E", "SE")
PLAYER_ACTIONS = (
    "idle", "walk", "sit", "pickup", "readyfight", "attack1", "hurt",
    "freeze", "die", "freeze2", "attack2", "attack3", "skill",
)
PLAYER_ACTION_COUNT = len(PLAYER_ACTIONS) * len(DIRECTIONS)


class FormatError(ValueError):
    """The supplied file or serialization input violates the supported format."""


def _integer(value: Any, low: int, high: int, label: str) -> int:
    if not isinstance(value, int) or not low <= value <= high:
        raise FormatError(f"{label} must be an integer in [{low}, {high}]")
    return value


def _i32(value: Any, label: str) -> int:
    return _integer(value, -(2**31), 2**31 - 1, label)


def _float(value: Any, label: str, positive: bool = False) -> float:
    try:
        result = float(value)
        representable = struct.unpack("<f", struct.pack("<f", result))[0]
    except (TypeError, ValueError, OverflowError, struct.error) as exc:
        raise FormatError(f"{label} must fit a finite float32") from exc
    if not math.isfinite(representable) or (positive and representable <= 0):
        raise FormatError(f"{label} must be finite" + (" and positive" if positive else ""))
    return result


def _emit(path: str | Path, data: bytes | bytearray) -> None:
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)


def _encode_zero_runs(pixels: bytes) -> bytes:
    result = bytearray()
    offset = 0
    while offset < len(pixels):
        value = pixels[offset]
        if value:
            result.append(value)
            offset += 1
            continue
        end = offset + 1
        while end < len(pixels) and not pixels[end] and end - offset < 255:
            end += 1
        result.extend((0, end - offset))
        offset = end
    if len(result) > 65535:
        raise FormatError("An indexed image's RLE payload exceeds SPR 2.1's 65535-byte limit")
    return bytes(result)


def write_spr(path: str | Path, images: Sequence[Mapping[str, Any]],
              palette: bytes | None = None) -> None:
    """Write SPR 2.1 from {width,height,pixels,sprite_type=0|1} dictionaries.

    Supply indexed images before truecolor images. A truecolor-only file may
    omit palette; a zero-filled unused palette is included for loader support.
    """
    indexed = sum(image.get("sprite_type", 0) == 0 for image in images)
    truecolor = len(images) - indexed
    _integer(indexed, 0, 65535, "indexed image count")
    _integer(truecolor, 0, 65535, "truecolor image count")
    if not images:
        raise FormatError("SPR must contain at least one image")
    if palette is None:
        if indexed:
            raise FormatError("Indexed images require a shared 1024-byte RGBA palette")
        palette = bytes(1024)
    if len(palette) != 1024:
        raise FormatError("SPR palette must contain exactly 1024 bytes")
    data = bytearray(struct.pack("<2sBBHH", b"SP", 1, 2, indexed, truecolor))
    for index, image in enumerate(images):
        kind = _integer(image.get("sprite_type", 0), 0, 1, "sprite_type")
        if kind != int(index >= indexed):
            raise FormatError("All indexed images must precede all truecolor images")
        width = _integer(image["width"], 1, 32767, "image width")
        height = _integer(image["height"], 1, 32767, "image height")
        pixels = bytes(image["pixels"])
        expected = width * height * (1 if kind == 0 else 4)
        if len(pixels) != expected:
            raise FormatError(f"Image {index}: expected {expected} pixel bytes, got {len(pixels)}")
        data.extend(struct.pack("<HH", width, height))
        if kind == 0:
            encoded = _encode_zero_runs(pixels)
            data.extend(struct.pack("<H", len(encoded)))
            data.extend(encoded)
        else:
            row_bytes = width * 4
            for row in range(height - 1, -1, -1):
                start = row * row_bytes
                for position in range(start, start + row_bytes, 4):
                    data.extend(pixels[position:position + 4][::-1])
    data.extend(palette)
    _emit(path, data)


def write_act(path: str | Path, actions: Sequence[Mapping[str, Any]],
              sounds: Sequence[str] = ()) -> None:
    """Write ACT 2.5 from ordered action/frame/layer dictionaries.

    action: {frames: [...], interval: 4.0}; frame: {layers: [...], anchors:
    [{x,y,unknown=0,other=0}], sound_id: -1}; layer: {sprite_index, width,
    height, sprite_type=0, x=0, y=0, mirror=False, color=[255]*4,
    scale_x=1, scale_y=1, rotation=0}. x/y are layer center offsets from the
    actor origin, with positive y downward. Supply anchors in the same space.
    """
    _integer(len(actions), 1, 65535, "action count")
    _integer(len(sounds), 0, 2**31 - 1, "sound count")
    sound_bytes = []
    for sound in sounds:
        encoded = sound.encode("ascii")
        if b"\0" in encoded or len(encoded) > 39:
            raise FormatError("Sound/event names must be at most 39 ASCII bytes without NUL")
        sound_bytes.append(encoded.ljust(40, b"\0"))
    data = bytearray(struct.pack("<2sBBH10x", b"AC", 5, 2, len(actions)))
    intervals = []
    for action_index, action in enumerate(actions):
        frames = action["frames"]
        _integer(len(frames), 1, 2**31 - 1, f"action {action_index} frame count")
        intervals.append(_float(action.get("interval", 4.0), "interval", positive=True))
        data.extend(struct.pack("<I", len(frames)))
        for frame in frames:
            layers = frame["layers"]
            _integer(len(layers), 0, 2**31 - 1, "layer count")
            data.extend(struct.pack("<32xI", len(layers)))
            for layer in layers:
                color = layer.get("color", (255, 255, 255, 255))
                if len(color) != 4:
                    raise FormatError("Layer color needs four RGBA bytes")
                color = tuple(_integer(channel, 0, 255, "color channel") for channel in color)
                mirror = layer.get("mirror", False)
                if mirror not in (False, True, 0, 1):
                    raise FormatError("mirror must be boolean")
                data.extend(struct.pack(
                    "<iiii4Bffiiii",
                    _i32(layer.get("x", 0), "layer x"),
                    _i32(layer.get("y", 0), "layer y"),
                    _integer(layer["sprite_index"], -1, 2**31 - 1, "sprite index"),
                    int(mirror), *color,
                    _float(layer.get("scale_x", 1.0), "scale_x", positive=True),
                    _float(layer.get("scale_y", 1.0), "scale_y", positive=True),
                    _i32(layer.get("rotation", 0), "rotation"),
                    _integer(layer.get("sprite_type", 0), 0, 1, "sprite_type"),
                    _integer(layer["width"], 0, 32767, "layer width"),
                    _integer(layer["height"], 0, 32767, "layer height"),
                ))
            sound_id = _integer(frame.get("sound_id", -1), -1, len(sounds) - 1, "sound_id")
            anchors = frame.get("anchors", [])
            _integer(len(anchors), 0, 2**31 - 1, "anchor count")
            data.extend(struct.pack("<iI", sound_id, len(anchors)))
            for anchor in anchors:
                data.extend(struct.pack(
                    "<Iiii", _integer(anchor.get("unknown", 0), 0, 2**32 - 1, "anchor unknown"),
                    _i32(anchor["x"], "anchor x"), _i32(anchor["y"], "anchor y"),
                    _i32(anchor.get("other", 0), "anchor other"),
                ))
    data.extend(struct.pack("<I", len(sound_bytes)))
    for name in sound_bytes:
        data.extend(name)
    for interval in intervals:
        data.extend(struct.pack("<f", interval))
    _emit(path, data)


class _Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    @property
    def remaining(self) -> int:
        return len(self.data) - self.offset

    def take(self, size: int) -> bytes:
        if size < 0 or size > self.remaining:
            raise FormatError(f"Truncated data at offset {self.offset}: need {size} bytes, have {self.remaining}")
        start = self.offset
        self.offset += size
        return self.data[start:self.offset]

    def field(self, fmt: str):
        values = struct.unpack("<" + fmt, self.take(struct.calcsize("<" + fmt)))
        return values[0] if len(values) == 1 else values

    def count(self, record_minimum: int, label: str) -> int:
        value = self.field("I")
        if value > self.remaining // record_minimum:
            raise FormatError(f"Impossible {label} count {value} at offset {self.offset - 4}")
        return value

    def finish(self) -> None:
        if self.remaining:
            raise FormatError(f"Unexpected trailing data: {self.remaining} bytes")


def _data(source: str | Path | bytes) -> bytes:
    return source if isinstance(source, bytes) else Path(source).read_bytes()


def read_spr(source: str | Path | bytes) -> dict[str, Any]:
    """Decode and validate SPR 2.1 independently of the writer's helpers."""
    reader = _Reader(_data(source))
    if reader.take(4) != b"SP\x01\x02":
        raise FormatError("Expected SPR signature and version 2.1")
    indexed, truecolor = reader.field("HH")
    if not indexed + truecolor:
        raise FormatError("SPR has no images")
    images = []
    for image_index in range(indexed + truecolor):
        width, height = reader.field("HH")
        if not 1 <= width <= 32767 or not 1 <= height <= 32767:
            raise FormatError(f"Image {image_index} has invalid dimensions")
        kind = int(image_index >= indexed)
        count = width * height
        if kind == 0:
            compressed = _Reader(reader.take(reader.field("H")))
            pixels = bytearray()
            while compressed.remaining:
                value = compressed.field("B")
                if value == 0:
                    run = compressed.field("B")
                    if run == 0:
                        raise FormatError("Noncanonical zero-length RLE run")
                    pixels.extend(bytes(run))
                else:
                    pixels.append(value)
                if len(pixels) > count:
                    raise FormatError("RLE expands past image bounds")
            if len(pixels) != count:
                raise FormatError("RLE does not fill the image")
        else:
            stored = reader.take(count * 4)
            pixels = bytearray(count * 4)
            for y in range(height):
                for x in range(width):
                    source_offset = ((height - y - 1) * width + x) * 4
                    destination_offset = (y * width + x) * 4
                    a, b, g, r = stored[source_offset:source_offset + 4]
                    pixels[destination_offset:destination_offset + 4] = bytes((r, g, b, a))
        images.append({"width": width, "height": height, "sprite_type": kind, "pixels": bytes(pixels)})
    palette = reader.take(1024)
    reader.finish()
    return {"version": "2.1", "indexed_count": indexed, "rgba_count": truecolor,
            "images": images, "palette": palette}


def read_act(source: str | Path | bytes) -> dict[str, Any]:
    """Decode ACT 2.5, checking truncation, counts, ranges and event references."""
    reader = _Reader(_data(source))
    if reader.take(4) != b"AC\x05\x02":
        raise FormatError("Expected ACT signature and version 2.5")
    action_count = reader.field("H")
    if not action_count:
        raise FormatError("ACT has no actions")
    reader.take(10)
    actions = []
    for action_index in range(action_count):
        frame_count = reader.count(44, "frame")
        if not frame_count:
            raise FormatError(f"Action {action_index} has no frames")
        frames = []
        for _ in range(frame_count):
            reader.take(32)
            layer_count = reader.count(44, "layer")
            layers = []
            for _ in range(layer_count):
                x, y, sprite_index, mirror = reader.field("iiii")
                color = reader.field("4B")
                scale_x, scale_y = reader.field("ff")
                rotation, sprite_type, width, height = reader.field("iiii")
                if sprite_index < -1 or mirror not in (0, 1) or sprite_type not in (0, 1):
                    raise FormatError("Invalid ACT layer index, mirror flag or image type")
                if not all(math.isfinite(v) and v > 0 for v in (scale_x, scale_y)):
                    raise FormatError("Invalid ACT layer scale")
                if not (0 <= width <= 32767 and 0 <= height <= 32767):
                    raise FormatError("Invalid ACT layer dimensions")
                layers.append(dict(x=x, y=y, sprite_index=sprite_index, mirror=bool(mirror),
                                   color=color, scale_x=scale_x, scale_y=scale_y,
                                   rotation=rotation, sprite_type=sprite_type, width=width, height=height))
            sound_id = reader.field("i")
            anchor_count = reader.count(16, "anchor")
            anchors = []
            for _ in range(anchor_count):
                unknown, x, y, other = reader.field("Iiii")
                anchors.append(dict(unknown=unknown, x=x, y=y, other=other))
            frames.append(dict(layers=layers, sound_id=sound_id, anchors=anchors))
        actions.append(dict(frames=frames))
    sound_count = reader.count(40, "sound")
    sounds = []
    for _ in range(sound_count):
        name = reader.take(40).split(b"\0", 1)[0]
        sounds.append(name.decode("ascii"))
    for action in actions:
        interval = reader.field("f")
        if not math.isfinite(interval) or interval <= 0:
            raise FormatError("Invalid ACT interval")
        action["interval"] = interval
        for frame in action["frames"]:
            if not -1 <= frame["sound_id"] < sound_count:
                raise FormatError("ACT frame references a nonexistent sound/event")
    reader.finish()
    return {"version": "2.5", "actions": actions, "sounds": sounds}


def validate_pair(spr_path: str | Path, act_path: str | Path,
                  require_player: bool = False) -> dict[str, Any]:
    """Read written files and cross-check every ACT image reference and size.

    This is structural validation, not proof of behavior in a Gravity client.
    require_player requires the standard 104 directions with at least one
    visible layer and body anchor in every frame.
    """
    spr = read_spr(spr_path)
    act = read_act(act_path)
    actions = act["actions"]
    if require_player and len(actions) != PLAYER_ACTION_COUNT:
        raise FormatError(f"Expected {PLAYER_ACTION_COUNT} player directional actions, got {len(actions)}")
    used = set()
    frame_total = 0
    layer_total = 0
    for ai, action in enumerate(actions):
        frame_total += len(action["frames"])
        for fi, frame in enumerate(action["frames"]):
            visible = False
            for layer in frame["layers"]:
                layer_total += 1
                index = layer["sprite_index"]
                if index == -1:
                    continue
                kind = layer["sprite_type"]
                count = spr["rgba_count"] if kind else spr["indexed_count"]
                if index >= count:
                    raise FormatError(f"Action {ai}, frame {fi}: image {kind}:{index} does not exist")
                absolute = index + (spr["indexed_count"] if kind else 0)
                image = spr["images"][absolute]
                if (layer["width"], layer["height"]) != (image["width"], image["height"]):
                    raise FormatError(f"Action {ai}, frame {fi}: ACT and SPR image dimensions differ")
                visible = visible or layer["color"][3] > 0
                used.add(absolute)
            if require_player and (not visible or not frame["anchors"]):
                raise FormatError(f"Action {ai}, frame {fi}: player frame needs a visible layer and body anchor")
    return {
        "valid": True, "validation_scope": "binary structure and cross-file references; no game-client execution",
        "spr_version": spr["version"], "act_version": act["version"],
        "indexed_images": spr["indexed_count"], "rgba_images": spr["rgba_count"],
        "actions": len(actions), "frames": frame_total, "layers": layer_total,
        "used_images": len(used), "unused_images": len(spr["images"]) - len(used),
        "player_layout": len(actions) == PLAYER_ACTION_COUNT,
        "spr_sha256": hashlib.sha256(Path(spr_path).read_bytes()).hexdigest(),
        "act_sha256": hashlib.sha256(Path(act_path).read_bytes()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    validate = sub.add_parser("validate", help="Validate an SPR/ACT pair")
    validate.add_argument("spr", type=Path)
    validate.add_argument("act", type=Path)
    validate.add_argument("--player", action="store_true", help="Require 104 actions and body anchors")
    args = parser.parse_args()
    try:
        print(json.dumps(validate_pair(args.spr, args.act, args.player), indent=2))
    except (FormatError, OSError, UnicodeError) as exc:
        parser.exit(1, f"Validation failed: {exc}\n")


if __name__ == "__main__":
    main()
