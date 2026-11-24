"""Convert each Frontier Tonfa hand with its original skin and reach motions."""
import argparse
import json
from pathlib import Path
import struct
import sys

import numpy as np


def converter(reference):
    kit = next((reference / "porting/third_party").glob("MHGU-Definitive*"))
    sys.path.insert(0, str(kit / "weapons/magnet_spike/tools"))
    import fmod2gu
    # Keep the existing viewer basis. The runtime wrist pose undoes this turn.
    fmod2gu.TURN = np.eye(3)[[1, 2, 0]]
    fmod2gu.VIEW_SHIFT = np.zeros(3)
    return fmod2gu


def hand_streams(raw, parser):
    groups = [[], []]
    pos = 12
    for _ in range(struct.unpack_from("<I", raw, 4)[0]):
        kind, records, size = struct.unpack_from("<3I", raw, pos)
        if kind == 2:
            offset = pos + 12
            for _ in range(records):
                mesh = parser._mesh(raw, offset)
                roots = {(mesh["bones"][b] if mesh["bones"] else b) // 9
                         for weights in mesh["weights"] for b, w in weights if w}
                if len(roots) != 1 or not roots <= {0, 1}:
                    raise ValueError("A Tonfa mesh must belong to a single hand")
                length, = struct.unpack_from("<I", raw, offset + 8)
                groups[roots.pop()].append(raw[offset:offset + length])
                offset += length
        pos += size
    for meshes in groups:
        if not meshes:
            raise ValueError("Missing Tonfa hand")
        data = b"".join(meshes)
        yield struct.pack("<6I", 1, 1, 24 + len(data), 2, len(meshes), 12 + len(data)) + data


def hands(converter, model, container, material):
    from catalog import member
    from jkr_decompress import decompress_jkr
    skeleton = decompress_jkr(member(container, 1))
    return [converter.build_mod(side, skeleton, material) for side in hand_streams(model, converter.fmod)]


def mechanism(blob):
    from retarget_tonfa import bank, channels, decode_clip, encode_tracks, euler
    slots = [bank(blob, side) for side in (0, 1)]
    assert len(slots[0]) == len(slots[1])
    out = bytearray(struct.pack("<4sHH", b"LMT\0", 67, len(slots[0])) + bytes(len(slots[0])*4))
    for slot, offsets in enumerate(zip(*slots)):
        if offsets == (None, None):
            continue
        sides = [decode_clip(blob, at, at + struct.unpack_from("<I", blob, at + 8)[0])["groups"]
                 for at in offsets]
        assert all(len(groups) == 6 for groups in sides)
        last = max(k[0] for groups in sides for g in groups for c in g["channels"] for k in c["keys"])
        times = np.arange(last + 1)
        tracks = {}
        for side, groups in enumerate(sides):
            for i, group in enumerate(groups):
                if not group["channels"]:
                    continue
                joint = 3 + 9*side + i
                tracks[0, joint] = euler(channels(group, times, 8, tangent_factor=1), (2, 1, 0))
                tracks[1, joint] = channels(group, times, 64, tangent_factor=1)
                scale = channels(group, times, 1, tangent_factor=1)
                if not np.allclose(scale, 1.):
                    tracks[2, joint] = scale
        start = encode_tracks(out, tracks)
        struct.pack_into("<I", out, 8 + 4*slot, start)
    return out


def repack(root, checkpoint, parser, database, cache, reference):
    sys.path[:0] = [str(checkpoint), str(parser / "mhfrontier/stage")]
    from build_model import archive, MOD, read_resources
    conv = converter(reference)
    native = root / "modules/Tonfa-Extreme/romfs/nativeNX"
    rows = json.loads((root / "data/catalog.json").read_text(encoding="utf8"))["families"]
    db = database.read_bytes()
    table, = struct.unpack_from("<I", db, 0x7c)
    variants = bytearray(len(rows))
    for row in rows:
        variant = db[table + 52 * row["frontier_row"] + 0x14]
        assert variant in (0, 1)
        variants[row["family"] - 1] = variant
        name = f"{row['asset']}.arc"
        archive_path = root / "data/models" / name
        parts = read_resources(archive_path)
        keys = sorted(key for key in parts if key[1] == MOD)
        assert len(keys) == 2 and "_l" in keys[0][0] and "_r" in keys[1][0]
        model = parts[keys[0]]
        at, = struct.unpack_from("<I", model, 44)
        material = model[at:at+128].split(b"\0")[0].decode("ascii")
        source = cache / str(row["frontier_model"])
        for key, mesh in zip(reversed(keys), hands(conv, source.with_suffix(".fmod").read_bytes(),
                                        source.with_suffix(".container").read_bytes(), material), strict=True):
            parts[key] = mesh
        packed = archive(parts)
        archive_path.write_bytes(packed)
        (native / "arc/weapon/w11" / name).write_bytes(packed)
    (root / "src/style/mechanisms.bin").write_bytes(variants)
    for variant in (0, 1):
        motion = checkpoint / f"kit/Frontier/w11t{variant}.mot.bin"
        (native / f"player/mot/frontier_tonfa/mechanism{variant}.lmt").write_bytes(mechanism(motion.read_bytes()))


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    for arg in ("root", "checkpoint", "parser", "database", "cache", "reference"):
        cli.add_argument(arg, type=Path)
    repack(**vars(cli.parse_args()))
