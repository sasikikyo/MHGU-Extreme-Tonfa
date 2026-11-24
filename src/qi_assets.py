"""Build Dragon Qi profiles for GU body tables checked against Frontier."""
import argparse
from pathlib import Path
import re
import struct
import sys


def quest_heads(rom):
    """A wound objective supplies evidence that an anatomical head can break."""
    heads = set()
    for path in (rom / "eng/quest/questData").glob("*_eng.gmd"):
        sub = path.read_bytes().rstrip(b"\0").rsplit(b"\0", 1)[-1]
        if not re.match(rb"(Wound|Break) ", sub) or not re.search(
                rb"\b(head|horns?|crest|beak|ears?|jaw|fangs?|tusks?|feelers|scalp|crown)\b", sub):
            continue
        objective = rom / "loc/quest/questData" / (path.stem[:-4] + ".ext")
        kind, species = struct.unpack_from("<BH", objective.read_bytes(), 0x29)
        if kind == 8:
            heads.add(species)
    if not {1, 2, 8, 32} <= heads:
        raise ValueError("Missing GU wound objective data")
    return heads


def gu_profile(path, species, heads):
    from kit.build_stats import archive_index, extract
    archive = path.read_bytes()
    index = archive_index(archive)
    name = path.stem
    body = extract(archive, index[rf"enemy\body_data\{name}_bodydata", 0x1ac7b52d])
    tune = extract(archive, index[rf"enemy\dt_tune\{name}_dttune", 0x48e8ac29])
    count, = struct.unpack_from("<I", body, 8)
    if (body[:4] != b"BDD\0" or len(body) != 16+44*(count+1) or body[-44:-40] != b"\xff"*4
            or tune[:8] != b"DTT\0\x6c\xa0\x33\x01"):
        raise ValueError(f"Unknown GU damage layout for {name}")
    zones = {}
    for p in range(16, 16+44*count, 44):
        bone, zone, part, flags = struct.unpack_from("<H4xBxBxH", body, p)
        if bone == 255 or flags & 0x100 or part >= 8 or zone >= 8:
            continue
        zones.setdefault(part, set()).add(zone)
    hz = 0x8c + tune[8]
    ko = lambda z: tune[hz + z*10 + 8]
    # The user requested whole-body head Qi for raptor Dromes and Bulldrome.
    # This affects Qi only. GU still uses its own stun multipliers per hit.
    drome = species in (14, 15, 16, 30, 34)
    all_stun = drome or (bool(zones) and all(ko(z) for group in zones.values() for z in group))
    head_break = species in heads or (species & 255) in heads or (species & 255) in (12, 13)
    # A cut counter alone also occurs on Rajang's breakable tail. Require a
    # detached tail model as well, and send pulses to the actual GU receiver.
    tail_model = any(k == 0x58a15856 and n.startswith("enemy\\") and n.endswith("_tail") for n,k in index)
    # Duramboros's tail comes off with ordinary damage (receiver 126).
    # GU's ordinary part submission feeds that counter for every pulse.
    tails = {p for p in zones if tail_model and (tune[0x40+10*p] == 1
             or (species == 55 and p == 7 and tune[0x40+10*p] == 126))
             and struct.unpack_from("<h", tune, 0x42+10*p)[0] > 0}
    if len(tails) > 1:
        raise ValueError(f"Ambiguous sever receivers for {name}")
    row = bytearray(struct.pack("<H", species))
    for part in range(9):
        if part not in zones:
            fields = (255, 0, 0, 0., 0)
        elif part in tails:
            fields = (7, 200, 200, .05, 5)
        elif all_stun:
            fields = (0, 400, 400, .05 if part == min(zones) else 0., 1)
        elif head_break and 0 in zones[part]:
            fields = (5, 300, 300, .015, 1)
        else:
            fields = (0, 400, 400, .002, 4)
        row.extend(struct.pack("<BHHfB", *fields))
    return row


def build(emd, client, enemy, checkpoint, style):
    sys.path.insert(0, str(checkpoint))
    from build_model import read_resources
    data = emd.read_bytes()
    image = client.read_bytes()
    header, = struct.unpack_from("<I", image, 60)
    count, = struct.unpack_from("<H", image, header + 6)
    size, = struct.unpack_from("<H", image, header + 20)
    base, = struct.unpack_from("<I", image, header + 52)

    def offset(va):
        for i in range(count):
            rva, length, start = struct.unpack_from("<3I", image, header + 24 + size + 40*i + 12)
            if rva <= va-base < rva+length:
                return start + va-base-rva
        raise ValueError(f"Missing Frontier body table at {va:x}")

    profiles, mapping = struct.unpack_from("<2I", data, 0x28)
    factors, = struct.unpack_from("<I", data, 0x30)
    ranks = struct.unpack_from("<5I", data, profiles)
    result = bytearray()
    # Packed GU quest species (variant in bits 8..11), Frontier mhfemd row.
    # Each variant must pass the collision check with its own two assets.
    for gu, frontier in ((1, 1), (2, 11), (0x201, 42), (0x202, 41), (14, 27), (15, 28)):
        # Frontier 82c6f0 uses 40-byte species records. GU stores the
        # corresponding hurt volumes as BDD rows with independent zone and part.
        pointer, = struct.unpack_from("<I", image, offset(0x118d7e88 + 40*frontier))
        p = offset(pointer)
        pairs = set()
        while struct.unpack_from("<h", image, p)[0] != -1:
            zone, part, flags = struct.unpack_from("<3H", image, p+4)
            if flags & 0x1604 == 0:
                pairs.add((part, zone))
            p += 40
        resources = read_resources(enemy / f"em{gu & 255:03}_{gu >> 8:02}.arc")
        body, = [raw for (name, _), raw in resources.items()
                 if raw.startswith(b"BDD\0") and "cam" not in name.lower()]
        parts = set()
        count_body, = struct.unpack_from("<I", body, 8)
        if len(body) != 16 + 44*(count_body+1) or body[-44:-40] != b"\xff"*4:
            raise ValueError(f"Unexpected GU body table for species {gu}")
        volumes = set()
        for p in range(16, 16 + 44*count_body, 44):
            bone, = struct.unpack_from("<H", body, p)
            flags, = struct.unpack_from("<H", body, p+10)
            if flags & 0x100 or (bone == 255 and flags == 0x12):
                continue
            part, zone = body[p+8], body[p+6]
            volumes.add((bone, part, zone))
        # Velocidrome and Gendrome have one whole-monster receiver in
        # Frontier. GU splits it into two parts. Both build the same charge,
        # with GU part 0 receiving the single original burst.
        whole_monster = gu in (14, 15)
        if whole_monster and (pairs != {(0, 0)} or {(p, z) for _, p, z in volumes} != {(0, 0), (1, 1)}):
            raise ValueError(f"Changed whole-monster body layout for species {gu}")
        for bone, part, zone in volumes:
            # Both metal Raths have an extra tip hitzone on GU joint 144.
            # The same variant's ordinary tail volume uses that joint too.
            # Both feed raw part 7; preserve GU's distinct native hitzones.
            if gu in (0x201, 0x202) and (bone, part, zone) == (144, 7, 7) and (144, 7, 4) in volumes:
                zone = 4
            if not whole_monster and (part, zone) not in pairs:
                raise ValueError(f"Unmatched GU part {gu}/{part}/{zone}")
            parts.add(part)
        pools = struct.unpack_from("<9h", data, mapping + frontier*18)
        if whole_monster and pools != (0,) + (-1,)*8:
            raise ValueError(f"Changed Frontier whole-monster Qi receiver for species {frontier}")
        result.extend(struct.pack("<H", gu))
        for part in range(9):
            source_part = 0 if whole_monster else part
            pool = pools[source_part] if part in parts else -1
            if pool == -1:
                result.extend(struct.pack("<BHHfB", 255, 0, 0, 0.0, 0))
                continue
            # The four ordinary Frontier quest profiles agree for these
            # monsters. The fifth is a Frontier-specific quest variant.
            limits = {struct.unpack_from("<H16xH52xH", data, r + 90*frontier + 2*pool)
                      for r in ranks[:4]}
            if len(limits) != 1 or not 0 <= pool < 9:
                raise ValueError("Rank-specific Dragon Qi mapping required")
            first, step, effect = limits.pop()
            if first == 0:
                raise ValueError("Unusable Dragon Qi pool")
            # 856f72 maps the charged pool a second time before looking up
            # its fraction of the monster's maximum health at mhfemd +30.
            index = pools[pool]
            if not 0 <= index < 9:
                raise ValueError("Invalid discharge factor index")
            factor, = struct.unpack_from("<f", data, factors + 4*(9*frontier+index))
            if not 0 <= factor <= 1:
                raise ValueError("Invalid Dragon Qi discharge factor")
            burst_factor = factor if part == source_part else 0.0
            result.extend(struct.pack("<BHHfB", pool, first, step, burst_factor, 1 if whole_monster else effect))
    heads = quest_heads(enemy.parents[1])
    for path in sorted(enemy.glob("em[0-9][0-9][0-9]_??.arc")):
        base_id, variant = map(int, path.stem[2:].split("_"))
        species = base_id | variant << 8
        if species not in (1, 2, 0x201, 0x202, 14, 15):
            result.extend(gu_profile(path, species, heads))
    records = sorted((result[i:i+92] for i in range(0, len(result), 92)),
                     key=lambda row: int.from_bytes(row[:2], "little"))
    (style / "qi-parts.bin").write_bytes(b"".join(records))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("emd", "client", "enemy", "checkpoint", "style"):
        parser.add_argument(arg, type=Path)
    build(**vars(parser.parse_args()))
