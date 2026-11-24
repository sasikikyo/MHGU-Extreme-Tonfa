"""Render the same move list shown in Forge using the player's positional bindings."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent
rows = (ROOT / "style/moves.txt").read_text(encoding="utf-8").splitlines()
fonts = Path("C:/Windows/Fonts")
def font(file, size):
    return ImageFont.truetype(str(fonts / file), size)

title = font("msyhbd.ttc", 61)
heading = font("msyhbd.ttc", 30)
chinese = font("msyh.ttc", 29)
english = font("segoeui.ttf", 23)
key = font("segoeui.ttf", 31)
groups = sum(row.startswith("#") for row in rows)
height = 255 + groups * 64 + (len(rows) - groups) * 85 + 125
im = Image.new("RGB", (2020, height), "#101b21")
d = ImageDraw.Draw(im)
d.rectangle((0, 0, 14, height), fill="#55ceb6")
d.text((62, 29), "穿龙棍  /  Extreme Style Tonfa", font=title, fill="#f5f2df")
d.text((65, 124), "LS = left stick    •    RB = right bumper    •    RT = right trigger    •    LMB / RMB / MMB = left / right / middle mouse button", font=key, fill="#acccc8")
d.text((65, 171), "Move with WASD. Hold a direction for directional evades. EX Evade and EX thrust require one full upper segment.", font=key, fill="#acccc8")
y = 239
for row in rows:
    if row.startswith("#"):
        zh, en = row[1:].split("|")
        d.rectangle((30, y, 1991, y+61), fill="#25433f")
        for x, label in [(61, zh+"  /  "+en), (750, "Xbox"), (1370, "键盘与鼠标  /  Keyboard & mouse")]:
            d.text((x, y+10), label, font=heading, fill="#e0f4df")
        y += 64
        continue
    zh, en, xbox, keyboard = row.split("|")
    d.text((64, y+6), zh, font=chinese, fill="#f7edd1")
    d.text((65, y+45), en, font=english, fill="#b4c6cb")
    for x, end, value in [(750, 1350, xbox), (1370, 1985, keyboard)]:
        assert d.textlength(value, font=key) < end-x, value
        d.text((x, y+21), value, font=key, fill="#e7edf0")
    d.line((60, y+82, 1960, y+82), fill="#34444a", width=1)
    y += 85
d.text((64, y+24), "RB + X = guard + sheathe. RT is the alternate EX input. Press one aerial boost per jump; land to reset.", font=key, fill="#a9dbcc")
im.save(ROOT.parent / "Extreme-Tonfa-Xbox-Keyboard.png", optimize=True)
