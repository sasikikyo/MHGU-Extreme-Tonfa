"""Sample Frontier's two weapon pivot tracks in the retargeted hunter wrist space."""
import argparse
from pathlib import Path
import struct
import sys
import numpy as np


def build(motion, checkpoint, client, output):
    sys.path.insert(0, str(checkpoint))
    from retarget_tonfa import bank, channels, decode_clip, euler, multiply
    # mhfo.b6f9a0 selects these four records for Tonfa. The joint is a
    # Frontier body node. Positions and XYZ Euler angles are local to it.
    exe = client.read_bytes()
    pe, = struct.unpack_from("<I", exe, 0x3c)
    n, = struct.unpack_from("<H", exe, pe+6)
    opt, = struct.unpack_from("<H", exe, pe+20)
    rva = 0x18720a8
    for idx in range(n):
        size, addr, _, file_at = struct.unpack_from("<4I", exe, pe+24+opt+40*idx+8)
        if addr <= rva < addr+size:
            off = file_at + rva-addr
            break
    else:
        raise ValueError("Missing Frontier Tonfa attachment records")
    mounts = [struct.unpack_from("<I7f", exe, off+32*i) for i in range(4)]
    if [m[0] for m in mounts] != [14, 18, 2, 2]:
        raise ValueError("Unexpected Frontier Tonfa joint IDs")
    basis = np.array([.5, .5, .5, .5])
    rotations = [euler(np.array([m[5:]]), (2, 1, 0))[0] for m in mounts]
    blob = motion.read_bytes()
    table = bytearray((300 * 2 + 4) * 8)
    samples = bytearray()
    seen = {}
    for section in range(6):
        for slot, start in enumerate(bank(blob, section)):
            if start is None:
                continue
            size, = struct.unpack_from("<I", blob, start + 8)
            groups = decode_clip(blob, start, start + size)["groups"]
            if len(groups) != 3:
                raise ValueError("Unexpected Tonfa pivot hierarchy")
            group = groups[2]
            last = max((k[0] for ch in group["channels"] for k in ch["keys"]), default=0)
            times = np.arange(int(last) + 1)
            rotation = euler(channels(group, times, 8, tangent_factor=1), (2, 1, 0))
            # Models store (source Y, Z, X). Restore source coordinates before
            # applying the moving pivot, so the hunter and grip use one space.
            pivot = channels(group, times, 64, tangent_factor=1)
            hand = section % 2
            row = 2 * (100 * (section // 2) + slot) + hand
            bindings = [(hand, row)]
            if section < 2 and slot in (2, 3):
                # The two halves of draw and sheathe share the original wrist
                # pivot track, even while the root is still on the pelvis.
                bindings.append((2 + hand, 600 + 2 * (slot - 2) + hand))
            for record, row in bindings:
                mount = rotations[record]
                local = multiply(multiply(mount, rotation), basis)
                cross = 2 * np.cross(mount[:3], pivot)
                rotated = pivot + mount[3]*cross + np.cross(mount[:3], cross)
                position = mounts[record][2:5] + mounts[record][1]*rotated
                poses = np.column_stack((position, local)).astype("<f4")
                if np.allclose(poses, poses[0], atol=1e-6, rtol=0):
                    poses = poses[:1]
                raw = poses.tobytes()
                if raw not in seen:
                    seen[raw] = len(table) + len(samples)
                    samples.extend(raw)
                struct.pack_into("<II", table, row * 8, seen[raw], len(poses))
    payload = table + samples
    for mount, rotation in zip(mounts, rotations):
        payload.extend(struct.pack("<8f", *mount[2:5], *multiply(rotation, basis), mount[1]))
    output.write_bytes(payload)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("motion", "checkpoint", "client", "output"):
        parser.add_argument(name, type=Path)
    build(**vars(parser.parse_args()))
