"""Compile Frontier ZZ Tonfa actions, their contact records and attached volumes."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

CONDITION = struct.Struct("<HBbHH")

def combo_attack_percentages(shell_file, levels):
    # Frontier 11e5800, shell class 100. Parameter row 136 gives the
    # inclusive range of rows, each holding four floats and four integers.
    header = struct.Struct("<hhHH5I")
    params = struct.Struct("<4f4i")
    with shell_file.open("rb") as stream:
        while record := stream.read(header.size):
            sub, kind, _, _, _, _, _, base, count = header.unpack(record)
            if kind == -1:
                break
            if (sub, kind) != (0, 100):
                continue
            if count <= 136:
                raise ValueError("Missing Tonfa combo parameter directory")
            stream.seek(base + 136 * params.size)
            first, last = params.unpack(stream.read(params.size))[4:6]
            if not (0 <= first <= last < count and 0 < levels <= 8 * (last - first + 1)):
                raise ValueError("Invalid Tonfa combo attack rows")
            stream.seek(base + first * params.size)
            rates = []
            while len(rates) < levels:
                rates.extend(params.unpack(stream.read(params.size)))
            if any(rate != int(rate) or not 0 < rate <= 65535 for rate in rates[:levels]):
                raise ValueError("Invalid Tonfa combo attack percentage")
            return [int(rate) for rate in rates[:levels]]
    raise ValueError("Missing Tonfa shell class 100")

def shockwave_volumes(volumes, skeleton):
    # All families share one combat rig. GU binds its hit volumes to the
    # hunter's hands, so express the shaft socket in its parent hand space.
    import numpy as np
    from mhfrontier.fmod.fskl import get_frontier_skeleton
    bones = get_frontier_skeleton(str(skeleton))
    result = []
    for joint, shape, zone, part, flags, radius, *xyz in volumes:
        if (joint, shape, flags) != (5, 1, 8):
            raise ValueError("Unexpected Tonfa shockwave attachment")
        points = np.array(xyz).reshape(2, 3)
        while joint != 2:
            if joint < 0:
                raise ValueError("Shockwave socket does not belong to the first hand")
            bone = bones[joint]
            q = np.array(bone.rotation)
            points *= bone.scale[:3]
            cross = 2 * np.cross(q[:3], points)
            points = points + q[3] * cross + np.cross(q[:3], cross) + bone.position[:3]
            joint = bone.parentID
        result.append((joint, shape, zone, part, flags, radius, *points.flat))
    return result

def pack_gauge_tuning(style, tuning, shell_file):
    fields = ("ex_size", "ex_levels", "combo_timeout_empty", "combo_timeout_full",
              "combo_hit_rate", "mode_zero_gain")
    rates = combo_attack_percentages(shell_file, tuning["ex_levels"])
    values = [*(tuning[name] for name in fields), *(hit for hit, _ in tuning["gain_rates"]),
              tuning["chain_cost"], *rates]
    (style / "gauge-tuning.bin").write_bytes(struct.pack(f"<{len(values)}H", *values))

def pack_flurry_power(style, tables):
    # 11e6190 replaces the raw shell power with byte 0 of a three-byte row.
    count, = tables.row("H", tables.pointer(0x10) + 0x392)
    rows = tables.block(count, tables.pointer(0x95c), 3)
    punches = bytearray()
    for at in rows:
        power, first_attribute, second_attribute = tables.row("3B", at)
        # GU writes the equipped element or ailment to one contact field.
        # ZZ gives both attributes the same percentage on every flurry row.
        if power == 0 or first_attribute != second_attribute:
            raise ValueError("Unsupported Tonfa flurry row")
        punches.extend((power, first_attribute))
    if not punches:
        raise ValueError("Missing Tonfa flurry motion values")
    (style / "flurry-power.bin").write_bytes(punches)
    release_count, = tables.row("H", tables.pointer(0x10) + 0x390)
    minimum, = tables.row("h", tables.pointer(0x660) + 0x5ba)
    medium, high = tables.row("2h", tables.pointer(0x660) + 0x5c0)
    rows = tables.block(release_count, tables.pointer(0x958), 2)
    if not 0 < minimum <= medium < high < release_count:
        raise ValueError("Invalid Tonfa release threshold")
    (style / "flurry-release.bin").write_bytes(
        struct.pack("<3H", minimum, medium, high) + b"".join(tables.blob[at:at + 2] for at in rows))

def pack_evade_reward(style, tables):
    # 1317050 checks the Tonfa directory first, then the common actions.
    # The first matching group/action row supplies flag 0 at byte 14.
    eligible = {}
    directory = tables.pointer(0x8c0)
    for offset in (0x58, 0x70):
        count, pointer = tables.row("II", directory + offset)
        for at in tables.block(count, pointer, 16):
            group, action = tables.row("BxH", at)
            eligible.setdefault((group, action), bool(tables.blob[at + 14] & 1))
    actions = bytes(sorted(action for (group, action), enabled in eligible.items()
                           if enabled and group == 1))
    tuning = tables.gauge()
    if not actions or any(eligible[key] for key in eligible if key[0] != 1):
        raise ValueError("Unsupported Frontier evade reward actions")
    evade_rates = [rate for _, rate in tuning["gain_rates"]]
    if len(evade_rates) <= tuning["ex_levels"] or len(evade_rates) > 255:
        raise ValueError("Invalid Extreme evade EX rate table")
    header = struct.pack("<3HB", tuning["special_evade_boost_frames"],
                         tuning["special_evade_boost_per_mille"], tuning["combo_evade_rate"],
                         len(evade_rates))
    (style / "evade-reward.bin").write_bytes(
        header + struct.pack(f"<{len(evade_rates)}H", *evade_rates) + actions)

def pack_gauge_art(style, source):
    from PIL import Image
    from itertools import groupby
    for name in ("combo", "ex", "timer"):
        with Image.open(source / f"tonfa-{name}.png") as texture:
            image = texture.convert("RGBA")
        spans = bytearray()
        for y in range(image.height):
            x = 0
            for color, pixels in groupby(image.getpixel((i, y)) for i in range(image.width)):
                end = x + sum(1 for _ in pixels)
                if color[3]:
                    spans.extend(bytes((x, end, y, 0, *color)))
                x = end
        (style / f"gauge-{name}.bin").write_bytes(spans)


def polish_discharge(original, wave):
    """Keep a quiet impact with a short, rounded decay, at the original pitch."""
    import numpy as np
    samples = original.astype(np.float64)
    def lowpass(hz, taps):
        t = np.arange(taps) - taps // 2
        kernel = np.sinc(2 * hz / 48000 * t) * np.hanning(taps)
        kernel /= kernel.sum()
        return np.convolve(np.pad(samples, taps // 2, mode="edge"), kernel, mode="valid")
    # Symmetric filtering keeps the transient on its authored animation frame.
    body = 0.9 * lowpass(2100, 129) + 0.1 * samples - 0.8 * lowpass(95, 1025)
    seconds = np.arange(len(body)) / 48000
    attack = np.minimum(seconds / 0.012, 1)
    decay = np.exp(-np.maximum(seconds - 0.18, 0) / 0.40)
    # Leave headroom for simultaneous detonations on two tagged monsters.
    ceiling_db = -18 if wave == 5 else -22
    ceiling = 32768 * 10**(ceiling_db / 20)
    return np.rint(ceiling * np.tanh(body / 24576) * attack * decay).astype("<i2")


def pack_sound(checkpoint, rom):
    from build_model import read_resources
    from convert_tonfa_audio import check_dsp, decode_ogg

    stage = checkpoint / "trial/frontier-sound/class"
    audio = checkpoint / "integration/audio"
    metadata = json.loads((audio / "class_cues.json").read_text())
    releases = {11: 5, 24: 22, 25: 23, 26: 21}
    # Cue 11 is the discharge strike. The other three are delayed blasts.
    users = {row["cue"]: row["wave"] for row in metadata["cues"]
             if row["cue"] in releases or row["wave"] in releases.values()}
    if users != releases or metadata["source_bank"] != "we11.snd" or metadata["rate"] != 48000:
        raise ValueError("Unexpected Frontier release bank")
    pcm = decode_ogg(stage / "wave.ogg")
    waves = {row["wave"]: row for row in metadata["waves"]}
    extensions = {0x1bcc4966: "srqr", 0x15d782fb: "sbkr", 0x79c47b59: "adpcm"}
    with tempfile.TemporaryDirectory() as tmp:
        adjusted = {}
        for number in releases.values():
            row = waves[number]
            start, count = row["start_frame"], row["frames"]
            original = pcm[start:start + count]
            if len(original) != count or count < 3:
                raise ValueError("Incomplete Frontier release sample")
            smooth = polish_discharge(original, number)
            raw = Path(tmp) / "release.pcm"
            raw.write_bytes(smooth.tobytes())
            encoded = Path(tmp) / f"w{number:02}.adpcm"
            subprocess.run([stage / "TonfaDsp.exe", raw, encoded], check=True)
            data = encoded.read_bytes()
            if check_dsp(data, smooth) < 20:
                raise ValueError(f"Discharge wave {number} failed its DSP round trip")
            adjusted[f"sound/se/pl/divine_tonfa/class/w{number:02}"] = data
        for name in ("divine_tonfa", "divine_tonfa_class"):
            for (path, kind), data in read_resources(audio / f"{name}.arc").items():
                path = path.replace("\\", "/")
                dest = rom / (path + "." + extensions[kind])
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(adjusted.pop(path, data) if kind == 0x79c47b59 else data)
        if adjusted:
            raise ValueError("Release wave missing from GU sound archive")


def restore_loop_points(checkpoint, project):
    source = (checkpoint / "w11goku.mot.bin").read_bytes()
    dest = project / "modules/Tonfa-Extreme/romfs/nativeNX/player/mot/frontier_tonfa"
    for bank, slots in ((0, (8,)), (4, (3, 9))):
        _, table = struct.unpack_from("<II", source, bank * 8)
        path = dest / f"tonfa_{bank}.lmt"
        converted = bytearray(path.read_bytes())
        for slot in slots:
            clip, = struct.unpack_from("<I", source, table + 4 * slot)
            flags, restart = struct.unpack_from("<If", source, clip + 12)
            header, = struct.unpack_from("<I", converted, 8 + 4 * slot)
            frames, = struct.unpack_from("<I", converted, header + 8)
            if not flags & 1 or not restart.is_integer() or not 0 < restart < frames:
                raise ValueError(f"Invalid source loop for bank {bank} slot {slot}")
            struct.pack_into("<i", converted, header + 12, int(restart))
        path.write_bytes(converted)

def build(checkpoint, gameplay, shell_file, project):
    restore_loop_points(checkpoint, project)
    sys.path.insert(0, str(checkpoint))
    import frontier_shapes
    from frontier_combat import Tables

    tables = Tables(gameplay.read_bytes())
    scripts = tables.scripts()
    shells = frontier_shapes.extract_shells(shell_file)
    if (len(scripts), len(shells["rows"])) != (153, 117):
        raise ValueError("Unexpected Frontier version")
    data = bytearray(24 * len(scripts))
    for state, script in enumerate(scripts):
        steps = [struct.pack("<hHHhHH", *s) for s in script["steps"]]
        branches = []
        for priority, condition, command, target, accept, close, execute, stop in script["cancels"]:
            branches.append(struct.pack("<3H", priority, condition, command)
                            + CONDITION.pack(*accept) + CONDITION.pack(*close)
                            + struct.pack("<H", target) + CONDITION.pack(*execute) + CONDITION.pack(*stop))
        events = [struct.pack("<HBb4H", *e) for e in script["events"]]
        for field, rows in enumerate((steps, branches, events)):
            struct.pack_into("<II", data, state * 24 + field * 8, len(rows), len(data))
            data.extend(b"".join(rows))
    style = project / "src/style"
    pack_gauge_tuning(style, tables.gauge(), shell_file)
    pack_flurry_power(style, tables)
    pack_evade_reward(style, tables)
    pack_gauge_art(style, checkpoint / "integration/hud")
    (style / "actions.bin").write_bytes(data)
    (style / "contacts.bin").write_bytes(b"".join(bytes.fromhex(r["raw"]) for r in shells["rows"]))
    pack_qi_gains(style, shells["rows"])
    pack_qi_timing(style, tables, shell_file)
    pack_sound_timelines(style, tables.sheet(0x61c, 22))
    rom = project / "modules/Tonfa-Extreme/romfs/nativeNX"
    shapes = rom / "player/hit/frontier_tonfa"
    shapes.mkdir(parents=True, exist_ok=True)
    for group, slots in enumerate(shells["groups"]):
        if group == 0:
            continue
        for name, slot in (("long", 2), ("short", 0)):
            volumes = slots[slot]
            if group == 10:
                volumes = shockwave_volumes(volumes, checkpoint / "kit/Frontier/wk480.fskl")
            (shapes / f"{name}{group}.hts").write_bytes(frontier_shapes.hts([volumes]))

    pack_sound(checkpoint, rom)


def pack_sound_timelines(style, sounds):
    # Clock uses four weighted requests per row. The reach condition is
    # resolved at build time into a timeline for each controller mode.
    for suffix, condition in (("long", 2), ("short", 1)):
        timeline = bytearray(500 * 4)
        for clip, events in sounds.items():
            if not 1000 <= clip < 1500:
                continue
            track = []
            for frame, kind, guard, *choices in events:
                if kind not in (3, 4, 6) or guard not in (0, condition):
                    continue
                options = [(min(c,255), w) for c,w in zip(choices[::2], choices[1::2]) if w]
                if not options:
                    options = [(min(choices[0],255),1)]
                # On release clips, the shared swings and mechanical transients
                # otherwise mask the quieter discharge. Keep its authored frame.
                if clip in (1418, 1429, 1463) and not (kind == 6 and options[0][0] == 11):
                    continue
                row = bytearray(struct.pack("<HBB", frame, int(kind!=6), len(options)))
                for cue, weight in options:
                    row.extend((cue,weight))
                track.append(bytes(row).ljust(12,b"\0"))
            struct.pack_into("<HH", timeline, 4*(clip-1000), len(timeline), len(track))
            timeline.extend(b"".join(track))
        (style / f"sound-{suffix}.bin").write_bytes(timeline)


def pack_qi_gains(style, rows):
    # 8a03b9 reads the second i32 of the shell's auxiliary record.
    gains = [row["parameters"][1] for row in rows]
    if len(gains) != 117 or any(not 0 <= g <= 255 for g in gains):
        raise ValueError("Unsupported Tonfa Dragon Qi contact gains")
    (style / "qi-gains.bin").write_bytes(bytes(gains))


def pack_qi_timing(style, tables, shell_file):
    data = shell_file.read_bytes()
    for pos in range(0, len(data)-27, 28):
        sub, kind, _, _, _, _, _, params, count = struct.unpack_from("<hhHH5I", data, pos)
        if kind == -1:
            break
        if (sub, kind) == (0, 100) and count > 95:
            delay, = struct.unpack_from("<I", data, params + 95*32 + 16)
            durations = tables.gauge()["resonance_frames"]
            (style / "qi-timing.bin").write_bytes(struct.pack("<4H", delay or 10, *durations))
            return
    raise ValueError("Missing class-100 discharge timing")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("gameplay", type=Path)
    parser.add_argument("shell_file", type=Path)
    parser.add_argument("--frontier-parser", type=Path, help="Directory containing the mhfrontier package")
    args = parser.parse_args()
    if args.frontier_parser:
        sys.path.insert(0, str(args.frontier_parser))
    build(args.checkpoint, args.gameplay, args.shell_file, Path(__file__).resolve().parents[1])
