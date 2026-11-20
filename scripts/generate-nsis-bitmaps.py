from PIL import Image, ImageDraw, ImageFont
from pathlib import Path

# 锚定脚本位置而非 CWD：从错误目录运行时，相对路径会静默建目录、把产物写错地方。
OUT_DIR = Path(__file__).resolve().parent.parent / "app" / "src-tauri" / "icons"
OUT_DIR.mkdir(parents=True, exist_ok=True)

PANEL_TOP = "#1d1d20"
PANEL_BOTTOM = "#0c0c0d"
ICON_PANEL = "#151517"
ICON_STROKE = "#2a2a2d"
# 眼睛与安装按钮都是品牌青柠（与 icon.svg 同色）；改这里要同步 installer.nsi 的 MEOWO_COL_LINK。
DOT_GREEN = "#d4f64c"
DOT_AMBER = "#d4f64c"
TEXT_COLOR = "#f5f5f7"
SUBTEXT_COLOR = "#a0a0a8"
# 一键安装背景的分区底色。改这两个值必须同步 installer.nsi 里 SetCtlColors 的
# 对应色值（MEOWO 标记块内有同名注释），否则控件文字会带色块。
BG_BASE = "#17171a"


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
    # 两只眼睛同色，中间要留缝才认得出是一双（半径 4、圆心距 12）。
    dot_r = 4
    dot_cy = iy + icon_size // 2 - 2
    paste_dot(img, ix + icon_size // 2 - 6, dot_cy, dot_r, DOT_GREEN)
    paste_dot(img, ix + icon_size // 2 + 6, dot_cy, dot_r, DOT_AMBER)

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
    cy = iy + icon_size // 2 - 4
    paste_dot(img, cx - 15, cy, 11, DOT_GREEN)
    paste_dot(img, cx + 15, cy, 11, DOT_AMBER)

    title_font = load_font("segoeui.ttf", 17)
    sub_font = load_font("NotoSansSC-VF.ttf", 11)
    draw.text((w // 2, iy + icon_size + 24), "Meowo", fill=TEXT_COLOR, font=title_font, anchor="mm")
    draw.text((w // 2, iy + icon_size + 48), "AI 会话看板", fill=SUBTEXT_COLOR, font=sub_font, anchor="mm")

    img.save(OUT_DIR / "nsis-sidebar.bmp", "BMP")


# 一键安装器的几何（逻辑 px，96dpi）。**必须与 installer.nsi 的 MEOWO_* 布局 define 一致**：
# 安装器把这些位图按 client 区等比缩小采样，原生控件按同一组坐标摆在烙好的形状上。
ONECLICK_W, ONECLICK_H = 640, 400        # 收起态 client 区
ONECLICK_EXPAND = 92                      # 展开「自定义安装选项」时窗口向下长出的高度
ICON_TOP, ICON_SIZE = 56, 72
BTN_X, BTN_Y, BTN_W, BTN_H = 188, 240, 264, 44
CARD_X, CARD_Y, CARD_W, CARD_H = 32, 340, 576, 128
FIELD_X, FIELD_Y, FIELD_W, FIELD_H = 52, 378, 536, 34
TOGGLE_SIZE = 18          # 勾选框边长（逻辑 px）；位置由 installer.nsi 摆
# 勾选框按常见显示缩放各出一套，安装器按 DPI 取对应那张原尺寸贴上，不拉伸（拉伸缩小会糊边）。
TOGGLE_DPIS = (96, 120, 144, 168, 192, 240, 288)
TOGGLE_ON_INK = "#1c2600"  # 同步 installer.nsi MEOWO_COL_ONLINK
CARD_FILL = "#1d1d21"      # 同步 installer.nsi MEOWO_COL_CARD
CARD_LINE = "#28282d"
FIELD_FILL = "#141416"     # 同步 installer.nsi MEOWO_COL_FIELD
FIELD_LINE = "#2e2e34"
SCALE = 2


def _px(v):
    return int(round(v * SCALE))


def _brand(img):
    """图标。字标「Meowo」不烙图，由安装器用 Segoe UI 实时渲染：底图按显示缩放缩小采样时
    细字边缘会出锯齿（150% 实拍），字体渲染在任何缩放下都清晰。"""
    size = _px(ICON_SIZE)
    icon = Image.open(OUT_DIR / "icon.png").convert("RGBA").resize((size, size), Image.LANCZOS)
    cx = img.width // 2
    img.paste(icon, (cx - size // 2, _px(ICON_TOP)), icon)


def _button(img):
    """主按钮：整条胶囊（圆角 = 半高）。安装器在胶囊直边区叠一条同色可点文字带。"""
    draw = ImageDraw.Draw(img)
    x, y, w, h = _px(BTN_X), _px(BTN_Y), _px(BTN_W), _px(BTN_H)
    draw.rounded_rectangle([x, y, x + w - 1, y + h - 1], radius=h // 2, fill=DOT_GREEN)


def _rounded(draw, x, y, w, h, r, fill, line):
    draw.rounded_rectangle([_px(x), _px(y), _px(x + w) - 1, _px(y + h) - 1], radius=_px(r), fill=fill, outline=line, width=SCALE)


def make_oneclick_bg():
    """一键安装器底图（2x，安装器 LoadImage 缩小采样）。三张：

    - nsis-oneclick-bg.bmp           进度页：品牌区，无按钮
    - nsis-oneclick-bg-btn.bmp       主页收起态 / 完成页：品牌区 + 主按钮
    - nsis-oneclick-bg-expanded.bmp  主页展开态（窗口加高 ONECLICK_EXPAND）：再加选项卡片、
                                     路径输入框外框、两个勾选框外框

    纯平底色、不画装饰：NSIS 原生控件的文字底色只能 SetCtlColors 配一个纯色，控件落在哪块
    （主底 / 卡片 / 输入框）就配哪块的色，才不会露接缝。文字（多语言）一律不烙图。
    2026-11-20 重做：原 780x512 窗口内容挤在上半、角落圆弧像脏块、展开区是系统白底控件（leader：「还是很丑」）。
    """
    base = Image.new("RGB", (_px(ONECLICK_W), _px(ONECLICK_H)), BG_BASE)
    _brand(base)
    base.save(OUT_DIR / "nsis-oneclick-bg.bmp", "BMP")
    _button(base)
    base.save(OUT_DIR / "nsis-oneclick-bg-btn.bmp", "BMP")

    tall = Image.new("RGB", (_px(ONECLICK_W), _px(ONECLICK_H + ONECLICK_EXPAND)), BG_BASE)
    _brand(tall)
    _button(tall)
    draw = ImageDraw.Draw(tall)
    _rounded(draw, CARD_X, CARD_Y, CARD_W, CARD_H, 12, CARD_FILL, CARD_LINE)
    _rounded(draw, FIELD_X, FIELD_Y, FIELD_W, FIELD_H, 8, FIELD_FILL, FIELD_LINE)
    tall.save(OUT_DIR / "nsis-oneclick-bg-expanded.bmp", "BMP")


def make_toggles():
    """勾选框两态（未选 = 深色圆角框；选中 = 青柠圆角块 + 深色对勾），底色是卡片色。

    选中态必须整块带圆角：此前选中是一块直角色块叠在圆角外框里（leader 2026-11-20：「勾选了之后
    没有圆角」）。4 倍超采样画好再 LANCZOS 缩到目标尺寸，圆角与对勾边缘是平滑的。
    """
    out = OUT_DIR / "nsis-ui"
    out.mkdir(exist_ok=True)
    for dpi in TOGGLE_DPIS:
        size = round(TOGGLE_SIZE * dpi / 96)
        k = 4
        big = size * k
        for state in ("on", "off"):
            img = Image.new("RGB", (big, big), CARD_FILL)
            d = ImageDraw.Draw(img)
            radius = round(5 * dpi / 96 * k)
            if state == "on":
                d.rounded_rectangle([0, 0, big - 1, big - 1], radius=radius, fill=DOT_GREEN)
                w = max(k * 2, round(2 * dpi / 96 * k))
                pts = [(big * 0.26, big * 0.52), (big * 0.43, big * 0.69), (big * 0.75, big * 0.33)]
                d.line(pts, fill=TOGGLE_ON_INK, width=w, joint="curve")
                for x, y in (pts[0], pts[-1]):
                    d.ellipse([x - w / 2, y - w / 2, x + w / 2, y + w / 2], fill=TOGGLE_ON_INK)
            else:
                line = max(k, round(dpi / 96 * k))
                d.rounded_rectangle([0, 0, big - 1, big - 1], radius=radius, fill=FIELD_FILL, outline=FIELD_LINE, width=line)
            img.resize((size, size), Image.LANCZOS).save(out / f"toggle-{state}-{dpi}.bmp", "BMP")


if __name__ == "__main__":
    make_header()
    make_sidebar()
    make_oneclick_bg()
    make_toggles()
    print("NSIS bitmaps updated.")
