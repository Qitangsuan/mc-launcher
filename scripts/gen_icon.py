"""生成启动器图标（1024 画布 → 各尺寸 + ico）。"""
import os
from PIL import Image, ImageDraw

OUT = r"C:\Users\祁棠夙安\Desktop\项目\mc-launcher\src-tauri\icons"
os.makedirs(OUT, exist_ok=True)

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# 圆角深色背景
d.rounded_rectangle([0, 0, S - 1, S - 1], radius=230, fill=(15, 23, 42, 255))

# 草方块
size = 640
x0 = (S - size) // 2
y0 = (S - size) // 2

# 土块本体
d.rectangle([x0, y0, x0 + size, y0 + size], fill=(134, 96, 67, 255))

# 草皮（上 1/3）
grass_h = size // 3
d.rectangle([x0, y0, x0 + size, y0 + grass_h], fill=(76, 175, 80, 255))

# 草皮下的高光/阴影像素
d.rectangle([x0, y0 + grass_h - 8, x0 + size, y0 + grass_h], fill=(56, 142, 60, 255))

# 像素格子纹理（4x4）
pix = size // 8
for i in range(8):
    for j in range(8):
        cx = x0 + i * pix
        cy = y0 + j * pix
        if j * pix < grass_h:
            shade = ((i * 37 + j * 13) % 17) - 8
            c = (max(0, min(255, 76 + shade)), max(0, min(255, 175 + shade)), max(0, min(255, 80 + shade)), 255)
        else:
            shade = ((i * 53 + j * 31) % 19) - 9
            c = (max(0, min(255, 134 + shade)), max(0, min(255, 96 + shade)), max(0, min(255, 67 + shade)), 255)
        d.rectangle([cx + 2, cy + 2, cx + pix - 2, cy + pix - 2], fill=c)

# 左上格线加一点立体感
d.rectangle([x0, y0, x0 + size, y0 + size], outline=(30, 41, 59, 60), width=4)

# 镐子：简单斜十字
pick_color = (226, 232, 240, 255)
d.line([(S * 0.30, S * 0.72), (S * 0.70, S * 0.30)], fill=pick_color, width=46)
d.line([(S * 0.52, S * 0.22), (S * 0.78, S * 0.44)], fill=pick_color, width=40)
d.line([(S * 0.40, S * 0.22), (S * 0.62, S * 0.44)], fill=pick_color, width=40)

img.save(os.path.join(OUT, "icon.png"))

for px in (32, 128, 256, 512):
    img.resize((px, px), Image.LANCZOS).save(os.path.join(OUT, f"{px}x{px}.png"))

img.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "128x128@2x.png"))
img.resize((48, 48), Image.LANCZOS).save(os.path.join(OUT, "icon_48.png"))
img.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "icon.ico"),
                                            sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

print("generated:", ", ".join(sorted(os.listdir(OUT))))
