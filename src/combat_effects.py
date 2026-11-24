"""Build the Tonfa-only GU particle bank from the installed Frontier effect archive."""
import argparse
from functools import lru_cache
from pathlib import Path
import struct
import sys
from tempfile import TemporaryDirectory
from functools import partial
from unittest.mock import patch


def part(data, index):
    count, = struct.unpack_from("<I", data)
    offset, size = struct.unpack_from("<II", data, 4 + index * 8)
    if not (index < count and 4 + count * 8 <= offset <= len(data) - size and size):
        raise ValueError(f"Missing effect archive entry {index}")
    return data[offset:offset + size]


def pel_template(data):
    if data[:8] != b"PEL\0\x0f\0\0\0":
        raise ValueError("Unexpected GU PEL version")
    cursor = 8
    for table in range(2):
        count, = struct.unpack_from("<I", data, cursor)
        cursor += 4
        for _ in range(count):
            start = cursor + 1 + data[cursor]
            cursor = start + 162
            if table == 1 and struct.unpack_from("<H", data, start + 2 + 0x30)[0] == 2:
                return data[start:cursor]
    raise ValueError("Missing GU particle template")


def build(client, rom, reference, checkpoint, animation, project):
    sys.path[:0] = [str(reference / "weapons/magnet_spike/tools"), str(reference / "tools"),
                    str(checkpoint), str(animation / "mhfrontier/stage")]
    import mhffx2gu as fx
    import mhfmodel
    import mrl
    import numpy as np
    from jkr_decompress import decompress_jkr
    from kit.build_stats import archive_index, extract

    @lru_cache(None)
    def archive(name):
        data = (rom / "arc" / name).read_bytes()
        return data, archive_index(data)

    def read(name, path, kind=fx.T_EFL):
        data, index = archive(name)
        return extract(data, index[path, kind])

    prefix = r"effect\frontier_tonfa"
    fx.ATLAS = prefix + r"\combat_atlas"
    fx.efl_name = lambda bank, effect: prefix + fr"\{bank:x}_{effect:02x}"
    # Motion event type 3 posts these Tonfa effects at the hunter's world pose.
    # The separate successful-evade and detonation routes keep their indices.
    sys.path.insert(0, str(checkpoint))
    from frontier_combat import Tables
    motion = Tables((project.parents[1] / "work/mhfcn-tonfa/mhfdat.bin.decd.bin").read_bytes()).sheet(0x618, 16)
    cues = sorted((clip, frame, variant, x, y, z) for clip, rows in motion.items()
                  for frame, kind, bank, variant, joint, flags, x, y, z in rows
                  if bank == 0xfac and
                  ((kind == 3 and not any((joint, flags, x, y, z))) or
                   (1028 <= clip <= 1031 and kind == 6 and joint == 0)))
    effects = [(0xfac, variant) for variant in (30, 9, 7, 2, 31)]
    # 11e4c40 posts one of four flurry shells on the striking hand.
    effects.extend((0xfac, variant) for variant in range(13, 17))
    # 1322090 configures the five 0xfaa charge emitters; clip 1213 also
    # posts 0xfaa/39 on both grips at frame 8 during an air breakfall.
    effects.extend((0xfaa, variant) for variant in (52, 53, 54, 55, 56, 39))
    effects.extend((0xfac, variant) for variant in sorted({cue[2] for cue in cues}))
    from retarget_tonfa import BONES
    # Discharge wind-up and the flurry finishing punch have their own
    # joint-attached events, separate from the later monster detonation.
    attached = [(clip, frame, bank, variant, BONES[joint], x, y, z)
                for clip in (1418, 1463, 1473, 1477, 1497, 1429)
                for frame, kind, bank, variant, joint, flags, x, y, z in motion[clip]
                if (kind, bank, variant) in ((3, 0xfaa, 12), (3, 0xfaa, 13), (6, 0xfac, 6))]
    for _, _, bank, variant, *_ in attached:
        if (bank, variant) not in effects:
            effects.append((bank, variant))
    # 854f97 posts faa/14 at one monster joint per newly marked Qi pool.
    effects.append((0xfaa, 14))

    class TonfaBuilder(fx.Builder):
        def efl(self, sid, eid):
            if (sid, eid) != (0xfaa, 13):
                return super().efl(sid, eid)
            source_set = self.src.set(sid)
            original = fx.uv_frames
            def curve_frames(emitter, life_scale):
                if emitter["uv_mode"] != 7:
                    return original(emitter, life_scale)
                curve = emitter["uv_u"]
                keys = source_set.curve(curve)
                if not keys:
                    raise ValueError(f"Missing discharge UV curve {curve}")
                count = max(1, int(emitter["life"] * life_scale))
                frames = [source_set.curve_at(curve, age / life_scale)[:2] for age in range(count)]
                return frames, 1.
            with patch.object(fx, "uv_frames", curve_frames):
                return super().efl(sid, eid)

        # GU's effect-model material samples its texture even when it ignores
        # the unit RGB. Keep the original animated alpha and bake the fixed
        # hue of these four shells into their own textured meshes.
        @lru_cache(None)
        def tinted_image(self, image, rgb):
            texels = self.src.images[image].copy()
            texels[:, :, :3] = (texels[:, :, :3].astype(np.uint16)
                                * np.array(rgb, dtype=np.uint16) + 127) // 255
            self.src.images.append(texels)
            return len(self.src.images) - 1

        def model_unit(self, st, a, e, ls):
            bank, variant = self.cur_eff
            if bank != 0xfac or variant not in (14, 15, 16):
                return super().model_unit(st, a, e, ls)
            hues = {tuple(bytes.fromhex(color)[:3]) for _, color, _ in st.colours(e['c_col'])}
            if len(hues) != 1:
                raise ValueError(f"Expected a single flurry hue for variant {variant}")
            rgb, = hues
            def mesh(index):
                image = self.tinted_image(mhfmodel.image_of(self.src, index), rgb)
                return self.models.model((index, rgb), [mhfmodel.mesh_part(self.src, index)], image)
            with patch.object(self.models, "mesh", mesh):
                return super().model_unit(st, a, e, ls)

    def decode(data):
        return decompress_jkr(data) if data.startswith(b"JKR") else data

    with TemporaryDirectory() as tmp:
        source = Path(tmp)
        raw = decode(client.read_bytes())
        (source / "sets.bin").write_bytes(decode(part(raw, 18)))
        (source / "model0.fmod").write_bytes(decode(part(decode(part(raw, 0)), 0)))
        (source / "effect.bin.dec").write_bytes(raw)
        class GameFiles:
            def read(self, path):
                return (rom.parent / path.lstrip("/")).read_bytes()

        shader = rom / "system/app_shader/AppShaderPackage.mfx"
        with patch.object(fx, "Builder", TonfaBuilder), \
             patch.dict(mrl.PKG, {0x22: (shader, *mrl.PKG[0x22][1:])}), \
             patch.object(mhfmodel, "Models", partial(mhfmodel.Models, prefix=prefix + r"\mesh")):
            files, groups, report = fx.build_all(read, effects, str(source), GameFiles())
    if any("not converted:" in line for line in report):
        raise ValueError(report)
    base = pel_template(read("player/quest/w00.arc", r"effect\pel\pl\w00_000", 0x5a525c16))
    records, routes = [], bytearray()
    for effect in effects:
        routes.extend(struct.pack("<HH", len(records), len(groups[effect])))
        for efl, mask, offset in groups[effect]:
            name = efl.encode("ascii")
            rec = bytearray(base)
            struct.pack_into("<3f", rec, 2, *offset)
            struct.pack_into("<3f", rec, 2 + 0x10, 0, 0, 0)
            struct.pack_into("<3f", rec, 2 + 0x20, 1, 1, 1)
            struct.pack_into("<Hh", rec, 2 + 0x30, len(records), -1)
            struct.pack_into("<I", rec, 2 + 0x48, mask)
            records.append(bytes([len(name)]) + name + rec)
    pel = b"PEL\0" + struct.pack("<III", 15, 0, len(records)) + b"".join(records) + bytes(4)
    dest = project / "modules/Tonfa-Extreme/romfs/nativeNX"
    kinds = {fx.T_EFL: ".efl", fx.T_EAN: ".ean", fx.T_TEX: ".tex",
             mhfmodel.T_MOD: ".mod", mhfmodel.T_MRL: ".mrl"}
    for name, kind, data in files:
        path = (dest / name.replace("\\", "/")).with_suffix(kinds[kind])
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    (dest / "effect/frontier_tonfa/combat.pel").write_bytes(pel)
    (project / "src/style/effect-routes.bin").write_bytes(routes)
    (project / "src/style/effect-qi.bin").write_bytes(struct.pack("<H", effects.index((0xfaa, 14))))
    (project / "src/style/effect-motion.bin").write_bytes(b"".join(
        struct.pack("<HHH3h", clip, frame, effects.index((0xfac, variant)), x, y, z)
        for clip, frame, variant, x, y, z in cues))
    (project / "src/style/effect-joints.bin").write_bytes(b"".join(
        struct.pack("<HHHh3h", clip, frame, effects.index((bank, variant)), joint, x, y, z)
        for clip, frame, bank, variant, joint, x, y, z in attached))
    print("\n".join(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("client", "rom", "reference", "checkpoint", "animation", "project"):
        parser.add_argument(name, type=Path)
    build(**vars(parser.parse_args()))
