"""生成新图标：深色圆角底 + 白色 M + 绿色状态点（与界面 brand 标记一致）。"""
import os
from PIL import Image, ImageDraw, ImageFont

OUT = r"C:\Users\祁棠夙安\Desktop\项目\mc-launcher\src-tauri\icons"
os.makedirs(OUT, exist_ok=True)

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# 深色圆角底（#18181b，与界面 --black 一致）
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=232, fill=(24, 24, 27, 255))

# 白色 M —— 优先 Arial Bold，找不到就退回系统字体
font = None
for path in (
    r"C:\Windows\Fonts\arialbd.ttf",
    r"C:\Windows\Fonts\segoeuib.ttf",
    r"C:\Windows\Fonts\msyhbd.ttc",
):
    try:
        font = ImageFont.truetype(path, 540)
        break
    except Exception:
        continue
if font is None:
    font = ImageFont.load_default()

bbox = d.textbbox((0, 0), "M", font=font)
w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
x = (S - w) / 2 - bbox[0]
y = (S - h) / 2 - bbox[1] - 52  # 稍微上移，给右下角的点留位置
d.text((x, y), "M", font=font, fill=(255, 255, 255, 255))

# 右下角绿色状态点（与界面 --green 一致）
r = S * 0.058
cx, cy = S * 0.787, S * 0.787
d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=(34, 197, 94, 255))

img.save(os.path.join(OUT, "icon.png"))

for px in (32, 128, 256, 512):
    img.resize((px, px), Image.LANCZOS).save(os.path.join(OUT, f"{px}x{px}.png"))

img.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "128x128@2x.png"))
img.resize((48, 48), Image.LANCZOS).save(os.path.join(OUT, "icon_48.png"))
img.resize((256, 256), Image.LANCZOS).save(
    os.path.join(OUT, "icon.ico"),
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)

print("generated:", ", ".join(sorted(os.listdir(OUT))))
