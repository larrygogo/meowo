from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

OUT_DIR = Path("app/src-tauri/icons")
OUT_DIR.mkdir(parents=True, exist_ok=True)

PANEL_TOP = "#1d1d20"
PANEL_BOTTOM = "#0c0c0d"
ICON_PANEL = "#151517"
ICON_STROKE = "#2a2a2d"
DOT_GREEN = "#4ec9a5"
DOT_AMBER = "#e0a23c"
TEXT_COLOR = "#f5f5f7"
SUBTEXT_COLOR = "#a0a0a8"


def interpolate(c1, c2, t):
    t = max(0, min(1, t))
    def parse(c):
        return tuple(int(c[i:i+2], 16) for i in (1, 3, 5))
    a, b = parse(c1), parse(c2)
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def gradient_bg(size):
    img = Image.new("RGB", size)
    draw = ImageDraw.Draw(img)
    w, h = size
    for y in range(h):
        t = y / (h - 1) if h > 1 else 0
        draw.line([(0, y), (w, y)], fill=interpolate(PANEL_TOP, PANEL_BOTTOM, t))
    return img


def load_font(name, size):
    candidates = [
        Path("/c/Windows/Fonts") / name,
        Path("C:/Windows/Fonts") / name,
    ]
    for p in candidates:
        if p.exists():
            return ImageFont.truetype(str(p), size)
    return ImageFont.load_default()


def paste_dot(img, cx, cy, r, color):
    """Draw a flat dot on an RGB image."""
    draw = ImageDraw.Draw(img)
    draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)


def make_header():
    w, h = 150, 57
    img = gradient_bg((w, h))
    draw = ImageDraw.Draw(img)

    icon_size = 28
    ix = w - 14 - icon_size
    iy = (h - icon_size) // 2
    draw.rounded_rectangle(
        [ix, iy, ix + icon_size, iy + icon_size],
        radius=7,
        fill=ICON_PANEL,
        outline=ICON_STROKE,
        width=1,
    )
    dot_r = 5
    dot_cy = iy + icon_size // 2 - 2
    paste_dot(img, ix + icon_size // 2 - 5, dot_cy, dot_r, DOT_GREEN)
    paste_dot(img, ix + icon_size // 2 + 5, dot_cy, dot_r, DOT_AMBER)

    font = load_font("segoeui.ttf", 15)
    draw.text((14, (h - 15) // 2 + 1), "Meowo", fill=TEXT_COLOR, font=font)

    img.save(OUT_DIR / "nsis-header.bmp", "BMP")


def make_sidebar():
    w, h = 164, 314
    img = gradient_bg((w, h))
    draw = ImageDraw.Draw(img)

    icon_size = 80
    ix = (w - icon_size) // 2
    iy = 48
    draw.rounded_rectangle(
        [ix, iy, ix + icon_size, iy + icon_size],
        radius=icon_size // 5,
        fill=ICON_PANEL,
        outline=ICON_STROKE,
        width=1,
    )

    cx = ix + icon_size // 2
    cy = iy + icon_size // 2 - 10
    paste_dot(img, cx - 17, cy, 17, DOT_GREEN)
    paste_dot(img, cx + 17, cy, 17, DOT_AMBER)

    title_font = load_font("segoeui.ttf", 17)
    sub_font = load_font("NotoSansSC-VF.ttf", 11)
    draw.text((w // 2, iy + icon_size + 24), "Meowo", fill=TEXT_COLOR, font=title_font, anchor="mm")
    draw.text((w // 2, iy + icon_size + 48), "AI 会话看板", fill=SUBTEXT_COLOR, font=sub_font, anchor="mm")

    img.save(OUT_DIR / "nsis-sidebar.bmp", "BMP")


if __name__ == "__main__":
    make_header()
    make_sidebar()
    print("NSIS bitmaps updated.")
