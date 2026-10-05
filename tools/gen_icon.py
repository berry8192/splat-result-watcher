"""アプリのアイコン（icons/icon.ico・icon.png）を描く。

自作の図形だけ（黒地に黄色の丸と十字）。ゲームの絵やロゴは使っていない。
2026-10-03 に GUI を足したときに作ったもの（出どころの記録として、同じ絵を描き直せるようにしてある）。
"""
from PIL import Image, ImageDraw

img = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle((8, 8, 248, 248), radius=48, fill=(24, 24, 32, 255))
d.ellipse((48, 48, 208, 208), fill=(232, 240, 40, 255))
d.rectangle((96, 120, 160, 136), fill=(24, 24, 32, 255))
d.rectangle((120, 96, 136, 160), fill=(24, 24, 32, 255))
img.save("icons/icon.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
img.save("icons/icon.png")
print("icons/icon.ico, icons/icon.png")
