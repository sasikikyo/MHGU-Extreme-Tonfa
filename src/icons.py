"""Insert the supplied Tonfa icon into private cells of the localized GU atlases."""
import argparse
from pathlib import Path
import sys
import numpy as np
from PIL import Image, ImageOps


CELLS = ((232, 32, 16), (160, 448, 24))
ATLAS = r"GUI\99_texture\HD_cmn_icon_GSM_NOMIP"


def build(rom, magnet, checkpoint, reference, icon, out):
    converter = next((reference / "porting/third_party").glob("MHGU-Definitive*"))
    sys.path[:0] = [str(checkpoint), str(converter / "tools")]
    from build_model import archive, read_resources
    import tex
    with Image.open(icon) as source:
        source = source.convert("RGBA")
        art = ImageOps.grayscale(source).convert("RGBA")
        art.putalpha(source.getchannel("A"))
        art = art.crop(art.getbbox())
    for original in sorted(rom.glob("*/arc/resident_*.arc")):
        relative = original.relative_to(rom)
        inherited = magnet / relative
        resources = read_resources(inherited if inherited.exists() else original)
        key, = [key for key in resources if key[0].endswith(ATLAS)]
        before = np.asarray(tex.decode(resources[key]))
        after = before.copy()
        for x, y, size in CELLS:
            box = after[2*y:2*(y+size), 2*x:2*(x+size)]
            if box[...,3].max() != 0:
                raise ValueError(f"Icon cell occupied in {relative} at {(x,y)}")
            img = ImageOps.contain(art, (2*size, 2*size), Image.Resampling.LANCZOS)
            tile = Image.new("RGBA", (2*size,2*size))
            tile.alpha_composite(img, ((2*size-img.width)//2,(2*size-img.height)//2))
            box[:] = np.array(tile)
        encoded = tex.encode_gu(after, "rgba8", mips=1)
        if not np.array_equal(tex.decode(encoded), after):
            raise ValueError(f"Atlas conversion changed pixels in {relative}")
        resources[key] = encoded
        dest = out / relative
        dest.parent.mkdir(parents=True,exist_ok=True)
        dest.write_bytes(archive(resources))
        if read_resources(dest) != resources:
            raise ValueError(f"Invalid localized archive {relative}")
        loose = out / (key[0].replace(chr(92), "/") + ".tex")
        loose.parent.mkdir(parents=True,exist_ok=True)
        loose.write_bytes(encoded)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("rom","magnet","checkpoint","reference","icon","out"):
        parser.add_argument(arg,type=Path)
    build(**vars(parser.parse_args()))
