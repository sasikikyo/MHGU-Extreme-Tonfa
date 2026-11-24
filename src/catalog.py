"""Build the independent Tonfa catalog from GU, the Magnet Spike policy and Frontier assets."""
import argparse
import hashlib
import html
import io
import json
from pathlib import Path
import re
import struct
import sys

from PIL import Image


DIVINE = 137
MODEL_START = 501
# Verified Frontier weapon rows. Separate rare species and Deviants may use
# a related species only after every exact match has had a choice of model.
MONSTERS = {
    7: ("Khezu", 4989), 9: ("Zinogre", 4529), 19: ("Nargacuga", 4737),
    22: ("Uragaan", 4746), 27: ("Rathalos", 3663), 29: ("Seregios", 6317),
    38: ("Tigrex", 6007), 39: ("Brachydios", 4549), 53: ("Yian Kut-Ku", 3657),
    61: ("Gore Magala", 4950), 62: ("Shagaru Magala", 6039), 63: ("Kirin", 4842),
    67: ("Deviljho", 4537), 74: ("Amatsu", 6225), 120: ("Crimson Fatalis", 3683),
    121: ("White Fatalis", 3684), 125: ("Barioth", 4602),
}


def member(blob, index):
    start, size = struct.unpack_from("<II", blob, 4 + index * 8)
    return blob[start:start + size]


def records(blob, width):
    count, = struct.unpack_from("<I", blob, 4)
    if len(blob) != 8 + count * width:
        raise ValueError("Unexpected native record layout")
    return [bytearray(blob[i:i + width]) for i in range(8, len(blob), width)]


def strings(path):
    b = path.read_bytes()
    size, = struct.unpack_from("<I", b, 32)
    return b[-size:].decode("utf-8").split("\0")


def reference_names(folder):
    names = {}
    for page in sorted(folder.glob("tonfa*.htm")):
        markup = page.read_text(encoding="utf-8-sig")
        for key, name, rest in re.findall(
            r'<td id="l([\dA-F]{4})"[^>]*>([^<]*)(.*?)</tr>', markup, re.I | re.S
        ):
            mats = re.findall(r'<a href="\.\./sozai/[^"]+"[^>]*>([^<]+)', rest)
            names[int(key, 16)] = (html.unescape(name), [html.unescape(m) for m in mats])
    return names


def choose_models(cache, refs):
    models = json.loads((cache / "models.json").read_text(encoding="utf8"))
    audit = json.loads((cache / "audit.json").read_text())
    choices, used = {}, set()

    def take(family, model, reason):
        info = audit[str(model)]
        if info["geometry"] in used:
            return False
        used.add(info["geometry"])
        entry = models[str(model)]
        candidates = [(r, *refs[r]) for r in entry["rows"] if r in refs]
        # A model may be reused by event weapons. Prefer the donor's monster row.
        term = {"Yian Kut-Ku": "Kut-Ku", "White Fatalis": "WhT. Fata",
                "Crimson Fatalis": "Fatalis Flame", "Rathalos": "Los Great",
                "Brachydios": "Brach", "Shagaru Magala": "S.Magala"}.get(reason, reason)
        candidates.sort(key=lambda c: (not any(term.lower() in m.lower() for m in c[2][:3]),
                                        not bool(c[2]), c[0]))
        row, name, materials = candidates[0] if candidates else (
            entry["rows"][0], entry["names"][0], [])
        choices[family] = dict(frontier_model=model, frontier_weapon=name, frontier_row=row,
                               frontier_material_evidence=materials[:3], model_selection=reason,
                               frontier_sound=entry["sound"], geometry_sha256=info["geometry"])
        return True

    take(DIVINE, 6480, "Divine Caduceus")
    for family, (monster, model) in MONSTERS.items():
        take(family, model, monster)
    # The base and Stygian models both depict Zinogre. Reserve the latter for
    # the Thunderlord family if it has distinct geometry.
    take(103, 4855, "Zinogre relative (Stygian)")
    for family in range(1, DIVINE):
        if family not in choices:
            for model in sorted(map(int, audit)):
                if take(family, model, "distinct Frontier fallback"):
                    break
    if len(choices) != DIVINE or len(used) != DIVINE:
        raise ValueError(f"Only {len(used)} distinct paired meshes available")
    return choices


def build(rom, checkpoint, cache, reference, ferias, parser, out):
    sys.path[:0] = [str(checkpoint), str(parser), str(parser / "mhfrontier/stage")]
    from build_model import archive, diffuse, read_resources, MOD, MRL, TEX
    from weapon_models import converter, hands
    from kit.build_stats import remap, SHARP

    def unpack(path):
        return {name.rsplit("\\", 1)[-1]: raw for (name, _), raw in read_resources(path).items()}

    resident = unpack(rom / "loc/arc/resident.arc")
    smithy = unpack(rom / "arc/facility/blacksmith.arc")
    base = records(resident["weapon00BaseData"], 33)
    level = records(resident["weapon00LevelData"], 15)
    forge = records(smithy["weaponCreateW00"], 38)
    upgrade = records(smithy["weaponProcessW00"], 32)
    if tuple(map(len, (base, level, forge, upgrade))) != (137, 959, 95, 958):
        raise ValueError("Unsupported GU smithy tables")
    policy = json.loads((reference / "porting/balance-policy.json").read_text())
    for r in level:
        for off, key in ((8, "raw_percent"), (12, "element_percent")):
            value, = struct.unpack_from("<H", r, off)
            struct.pack_into("<H", r, off, (value * policy[key] + 50) // 100)
    donor_levels = resident["weapon11LevelData"]
    rustrazor = next(r for r in records(donor_levels, 17) if tuple(r[4:6]) == (107, 5))
    sharp = remap(donor_levels, resident["kireajiData"])[SHARP]
    ends = struct.unpack_from("<7H", sharp, len(sharp) - 14)
    by_name = strings(rom / "eng/table/itemData_eng.gmd")[::2]
    gem = by_name.index("Lrg Elder Dragon Gem")
    # Use a separate single-level, endgame entry with no parents or derivatives.
    b = bytearray(33)
    struct.pack_into("<IHH", b, 0, DIVINE, MODEL_START + DIVINE - 1, 144)
    donor_base = records(resident["weapon11BaseData"], 34)[107]
    b[9], b[11], b[12:16], b[22] = 1, donor_base[11], donor_base[12:16], donor_base[22]
    struct.pack_into("<H", b, 19, max(struct.unpack_from("<H", row, 19)[0] for row in base) + 1)
    base.append(b)
    l = bytearray(15)
    struct.pack_into("<IBB", l, 0, len(level), DIVINE, 1)
    l[6:14], l[14] = rustrazor[6:14], rustrazor[16]
    l[6] = (len(sharp) - 8) // 14 - 1
    level.append(l)
    c = bytearray(38)
    struct.pack_into("<HHHHBB", c, 0, 1, DIVINE, 0, gem, 10, 2)
    forge.append(c)
    u = bytearray(32)
    struct.pack_into("<HH", u, 0, DIVINE, 1)
    upgrade.append(u)

    out.mkdir(parents=True, exist_ok=True)
    models_dir = out / "models"
    models_dir.mkdir(exist_ok=True)
    base_model = read_resources(rom / "arc/weapon/w11/047.arc")
    conv = converter(reference)
    donor = base_model[r"weapon\w11\sou047\sou_r047", MOD]
    materials, meshes = struct.unpack_from("<II", donor, 44)
    mat_id = (struct.unpack_from("<H", donor, meshes + 3*48 + 5)[0] >> 4) & 0xfff
    mat_at = materials + 128*mat_id
    material = donor[mat_at:mat_at + 128].split(b"\0")[0].decode("ascii")
    choices = choose_models(cache, reference_names(ferias))
    names = strings(rom / "eng/table/weapon00MsgData_eng.gmd")
    family_rows = []
    for family, row in enumerate(base[1:], 1):
        pick = choices[family]
        model = pick["frontier_model"]
        package = (cache / f"{model}.container").read_bytes()
        right, left = hands(conv, (cache / f"{model}.fmod").read_bytes(), package, material)
        number = MODEL_START + family - 1
        struct.pack_into("<H", row, 4, number)
        resources = {}
        with Image.open(io.BytesIO(member(member(package, 2), 0))) as img:
            tex = diffuse(img)
        for (path, kind), raw in base_model.items():
            if path.startswith("weapon\\w11\\sou047"):
                if kind == TEX:
                    raw = tex
                elif kind == MRL:
                    raw = raw.replace(b"sou047", f"sou{number}".encode())
                elif kind == MOD:
                    raw = left if "_l047" in path else right
                path = path.replace("047", str(number))
            resources[path, kind] = raw
        dest = models_dir / f"{number}.arc"
        dest.write_bytes(archive(resources))
        if read_resources(dest) != resources:
            raise ValueError(f"Invalid converted model archive {number}")
        source = None if family == DIVINE else names[family * 6]
        family_rows.append(dict(family=family, asset=number, spike_donor=source, **pick))
    for name, data, rows in (("base", resident["weapon00BaseData"], base),
                             ("level", resident["weapon00LevelData"], level),
                             ("forge", smithy["weaponCreateW00"], forge),
                             ("upgrade", smithy["weaponProcessW00"], upgrade)):
        (out / f"{name}.bin").write_bytes(data[:4] + struct.pack("<I", len(rows)) + b"".join(rows))
    (out / "sharpness.bin").write_bytes(sharp[-14:])
    catalog = dict(families=family_rows, style="Extreme Tonfa", equipment_type=25,
                   source_balance=policy["revision"], direct_recipes=len(forge),
                   divine=dict(id=DIVINE, level=1, raw=struct.unpack_from("<H",l,8)[0],
                               affinity=l[11], slots=l[14], sharpness_endpoints=ends,
                               materials=[dict(name=by_name[gem], id=gem, quantity=10)]))
    (out / "catalog.json").write_text(json.dumps(catalog, ensure_ascii=False, indent=2), encoding="utf8")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("rom", "checkpoint", "cache", "reference", "ferias", "parser", "out"):
        p.add_argument(name, type=Path)
    build(**vars(p.parse_args()))
