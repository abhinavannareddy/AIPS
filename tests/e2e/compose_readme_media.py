"""Turns the raw captures from readme-media.mjs into the README banner, walkthrough GIF and
trimmed feature images. Usage: python tests/e2e/compose_readme_media.py <rawDir> [outDir]"""
import sys
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

RAW = Path(sys.argv[1])
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "docs/media")
OUT.mkdir(parents=True, exist_ok=True)
NAVY, PINK, WHITE, MUTED = (21, 32, 43), (214, 51, 132), (255, 255, 255), (168, 182, 196)
# shortcut: Windows font path; on Linux/macOS point this at any DIN-style TTF.
FONT = "C:/Windows/Fonts/bahnschrift.ttf"


def font(size, weight="Regular"):
    f = ImageFont.truetype(FONT, size)
    f.set_variation_by_name(weight)
    return f


def trim_bottom(im, margin=40):
    """Drop empty page background below the last content, ignoring the full-height sidebar."""
    body = im.crop((int(im.width * 0.2), 0, im.width, im.height)).convert("RGB")
    bg = Image.new("RGB", body.size, body.getpixel((body.width - 5, body.height - 5)))
    box = ImageChops.difference(body, bg).point(lambda v: 255 if v > 12 else 0).getbbox()
    return im if not box else im.crop((0, 0, im.width, min(im.height, box[3] + margin)))


def save_png(im, name, width=1600):
    im = im.convert("RGB")
    if im.width > width:
        im = im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    im.save(OUT / name, optimize=True)


def framed(im, width, radius=14):
    """Screenshot with rounded corners and a soft drop shadow, on a transparent canvas."""
    im = im.convert("RGB").resize((width, round(im.height * width / im.width)), Image.LANCZOS)
    mask = Image.new("L", im.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, *im.size), radius, fill=255)
    pad = 40
    canvas = Image.new("RGBA", (im.width + 2 * pad, im.height + 2 * pad), (0, 0, 0, 0))
    shadow = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (pad, pad + 12, pad + im.width, pad + im.height + 12), radius, fill=(0, 0, 0, 150))
    canvas = Image.alpha_composite(canvas, shadow.filter(ImageFilter.GaussianBlur(18)))
    canvas.paste(im, (pad, pad), mask)
    return canvas


def banner():
    W, H = 1600, 600
    im = Image.new("RGBA", (W, H), NAVY + (255,))
    d = ImageDraw.Draw(im)
    for x in range(0, W, 40):  # faint grid, like a chart plot
        d.line((x, 0, x, H), fill=(28, 42, 56))
    for y in range(0, H, 40):
        d.line((0, y, W, y), fill=(28, 42, 56))
    shot = Image.open(RAW / "step3-memory.png").crop((0, 230, 1880, 1530))  # below the dev-mode header
    im.alpha_composite(framed(shot, 860), (720, 70))
    d.text((70, 92), "AIRCRAFT INTELLIGENCE & PROTECTION SYSTEM", font=font(20, "Bold"), fill=PINK)
    d.text((66, 120), "AIPS", font=font(150, "Bold"), fill=WHITE)
    d.multiline_text((70, 300), "One evidence-graded memory per aircraft,\nfrom the first fault to the next departure.",
                     font=font(30), fill=WHITE, spacing=10)
    chips = ["Cross-Flight Memory", "Pre-departure assurance", "Guardian drills", "Tamper-evident audit"]
    x, y = 70, 420
    for c in chips:
        w = d.textlength(c, font=font(19, "SemiBold")) + 30
        if x + w > 680:
            x, y = 70, y + 50
        d.rounded_rectangle((x, y, x + w, y + 38), 19, outline=PINK, width=2)
        d.text((x + 15, y + 8), c, font=font(19, "SemiBold"), fill=WHITE)
        x += w + 12
    im.convert("RGB").save(OUT / "banner.png", optimize=True)


STEPS = [
    ("step1-fleet", "Fleet overview: 50 aircraft ranked by attention; SE-RBA is flagged \u201cReview required\u201d"),
    ("step2-aircraft", "Aircraft health: Hydraulics shows a Caution with remaining-life bands for each engine"),
    ("step3-memory", "Cross-Flight Memory: fault 29-31-04 came back 4 times in 14 days, after 2 repairs"),
    ("step4-case", "An investigation opens automatically, with the linked evidence attached"),
    ("step5-preflight", "Pre-departure report for NA1070 puts Hydraulics first. Advisory, never a go/no-go"),
    ("step6-audit", "The engineer's review is written to a hash-chained audit trail"),
]


def walkthrough():
    W, H, BAR = 1200, 750, 78
    frames = []
    for i, (name, caption) in enumerate(STEPS, 1):
        shot = Image.open(RAW / f"{name}.png").convert("RGB")
        shot = shot.crop((0, 0, shot.width, round(shot.width * H / W)))
        frame = Image.new("RGB", (W, H + BAR), NAVY)
        frame.paste(shot.resize((W, H), Image.LANCZOS), (0, 0))
        d = ImageDraw.Draw(frame)
        d.rounded_rectangle((24, H + 20, 104, H + 58), 19, fill=PINK)
        d.text((64, H + 39), f"{i} / {len(STEPS)}", font=font(20, "Bold"), fill=WHITE, anchor="mm")
        d.text((124, H + 39), caption, font=font(24), fill=WHITE, anchor="lm")
        frames.append(frame.quantize(colors=128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
    frames[0].save(OUT / "walkthrough.gif", save_all=True, append_images=frames[1:],
                   duration=3200, loop=0, optimize=True)


banner()
walkthrough()
for raw in sorted(RAW.glob("*.png")):
    save_png(trim_bottom(Image.open(raw)), raw.name)
print("written:", ", ".join(sorted(p.name for p in OUT.iterdir())))
