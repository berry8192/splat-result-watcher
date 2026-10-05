"""手がかりの数字（src/starter_digits.rs）を作る。`python tools/gen_starter.py`（repo の一番上で）

- BOLD: 利用者の手書き assets/hand_digits.png（git 管理外。黒で描いた所を、枠ごとに切り出す）
- GAUGE / MENU: 下の strokes() で線を描いたもの（Claude が描いた。MENU は横に 1.2 倍）

ゲームの数字と同じく、白の外接の高さを 30 にそろえ、横は比を保って真ん中に置く（src/matching.rs と同じ最近傍）。
"""
import os
import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
N = 30


def norm(b):
    ys, xs = np.nonzero(b)
    b = b[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    h, w = b.shape
    sw = max(1, min(N, round(w * N / h)))
    off = (N - sw) // 2
    out = np.zeros((N, N), np.uint8)
    for gy in range(N):
        for gx in range(sw):
            out[gy, off + gx] = b[gy * h // N, gx * w // sw]
    return out

H, W = 400, 200   # 字の高さと幅（縦長）
def strokes(d, ctx):
    l, r, t, b = 0, W, 0, H
    m = H / 2
    arc, line = ctx
    if d == 0:
        arc((l, t, r, b), 0, 360)
    elif d == 1:
        line([(W * 0.55, t), (W * 0.55, b)]); line([(W * 0.55, t), (W * 0.25, H * 0.15)])
    elif d == 2:
        arc((l, t, r, H * 0.5), 180, 360); arc((l, t, r, H * 0.5), 0, 40)
        line([(W * 0.88, H * 0.41), (l, b)]); line([(l, b), (r, b)])
    elif d == 3:
        arc((l, t, r, m), 200, 360); arc((l, t, r, m), 0, 90)
        arc((l, m, r, b), 270, 360); arc((l, m, r, b), 0, 160)
        line([(W * 0.4, m), (W * 0.5, m)])
    elif d == 4:
        line([(W * 0.75, t), (l, H * 0.7)]); line([(l, H * 0.7), (r, H * 0.7)]); line([(W * 0.75, t), (W * 0.75, b)])
    elif d == 5:
        line([(r, t), (W * 0.1, t)]); line([(W * 0.1, t), (W * 0.117, H * 0.474)])
        arc((l, H * 0.36, r, b), 220, 360); arc((l, H * 0.36, r, b), 0, 160)
    elif d == 6:
        arc((l, H * 0.4, r, b), 0, 360); arc((l, t, W * 1.9, H * 1.4), 180, 265)
    elif d == 7:
        line([(l, t), (r, t)]); line([(r, t), (W * 0.35, b)])
    elif d == 8:
        arc((W * 0.06, t, W * 0.94, m), 0, 360); arc((l, m, r, b), 0, 360)
    elif d == 9:
        arc((l, t, r, H * 0.6), 0, 360); arc((-W * 0.9, -H * 0.4, r, b), 0, 85)

def render(d, sw, wf=1.0):
    pad = sw + 4
    im = Image.new('L', (int(W * wf) + pad * 2, H + pad * 2), 0)
    dr = ImageDraw.Draw(im)
    def tr(p): return (p[0] * wf + pad, p[1] + pad)
    def arc(box, a0, a1):
        x0, y0 = tr((box[0], box[1])); x1, y1 = tr((box[2], box[3]))
        dr.arc((x0 + sw / 2, y0 + sw / 2, x1 - sw / 2, y1 - sw / 2), a0, a1, fill=255, width=sw)
    def line(pts):
        pts = [tr(p) for p in pts]
        pts = [(min(max(x, pad + sw / 2), im.width - pad - sw / 2), min(max(y, pad + sw / 2), im.height - pad - sw / 2)) for x, y in pts]
        dr.line(pts, fill=255, width=sw)
        for x, y in pts: dr.ellipse((x - sw / 2, y - sw / 2, x + sw / 2, y + sw / 2), fill=255)
    strokes(d, (arc, line))
    return (np.array(im) >= 128).astype(np.uint8)



def main():
    sets = {}
    im = np.array(Image.open(os.path.join(ROOT, 'assets', 'hand_digits.png')).convert('RGB')).astype(int)
    black = (im.max(-1) < 100).astype(np.uint8)
    # 下書きの枠: 余白 30、1 字 220×300
    sets['BOLD'] = [norm(black[30:330, 30 + i * 220 + 8:30 + (i + 1) * 220 - 8]) for i in range(10)]
    # 線の太さ 30（字の高さ 400 に対して）。横の倍率は手元の見本でいちばん合ったもの
    for name, wf in (('GAUGE', 1.0), ('MENU', 1.2)):
        sets[name] = [norm(render(d, 30, wf)) for d in range(10)]
    out = ['// 生成したもの（tools/gen_starter.py）。手で書き換えない。中身の説明は starter.rs']
    for name, gl in sets.items():
        out.append(f'pub const {name}: [[u32; 30]; 10] = [')
        for d, g in enumerate(gl):
            rows = [sum(int(g[y, x]) << x for x in range(N)) for y in range(N)]
            out.append(f'    // {d}')
            out.append('    [' + ', '.join(f'0x{r:08x}' for r in rows) + '],')
        out.append('];\n')
    path = os.path.join(ROOT, 'src', 'starter_digits.rs')
    open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))
    print('wrote', path)


if __name__ == '__main__':
    main()
