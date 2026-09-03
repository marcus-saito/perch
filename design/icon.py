"""Perch's icon: a wren on a rule.

    python3 design/icon.py 1024 icon.png
    cd app && npx tauri icon ../icon.png

The icon is drawn here rather than kept only as a PNG, so the shapes stay
editable and the palette stays the one in design/perch.css. Warm paper ground,
the terracotta accent used once, and the rule the bird stands on is the same
line a feed row sits on.

Pure standard library: a scanline polygon rasteriser with 4x vertical
supersampling and exact horizontal span coverage, plus a small PNG writer.
Perch gains no dependency for one asset.

The sizes that decide the design are the small ones. At 32 px the bird has to
still be a bird, which is what sets its weight against the tile and why the
rule is drawn as heavily as it is.
"""
import math, zlib, struct

PAPER = (0xFA, 0xF8, 0xF5)
TERRA = (0xA8, 0x55, 0x2F)
PERCHGREY = (0xA6, 0x99, 0x88)
EDGE = (0xE7, 0xDF, 0xD4)

def circle(cx, cy, r, n=192):
    return [(cx + r*math.cos(2*math.pi*i/n), cy + r*math.sin(2*math.pi*i/n)) for i in range(n)]

def ellipse(cx, cy, rx, ry, deg=0.0, n=192):
    a = math.radians(deg); ca, sa = math.cos(a), math.sin(a)
    pts = []
    for i in range(n):
        t = 2*math.pi*i/n
        x, y = rx*math.cos(t), ry*math.sin(t)
        pts.append((cx + x*ca - y*sa, cy + x*sa + y*ca))
    return pts

def round_rect(x0, y0, x1, y1, r, n=48):
    pts = []
    for (cx, cy, a0) in ((x1-r, y1-r, 0), (x0+r, y1-r, 90), (x0+r, y0+r, 180), (x1-r, y0+r, 270)):
        for i in range(n+1):
            a = math.radians(a0 + 90*i/n)
            pts.append((cx + r*math.cos(a), cy + r*math.sin(a)))
    return pts

def coverage(polys, w, h, sub=4):
    """Union coverage of polygons, 0..1 per pixel. Even-odd within a polygon,
    max across polygons so overlaps do not darken."""
    cov = [0.0]*(w*h)
    for poly in polys:
        edges = []
        for i in range(len(poly)):
            x0, y0 = poly[i]; x1, y1 = poly[(i+1) % len(poly)]
            if y0 != y1:
                edges.append((y0, y1, x0, x1))
        if not edges:
            continue
        ymin = max(0, int(min(min(e[0], e[1]) for e in edges)))
        ymax = min(h-1, int(max(max(e[0], e[1]) for e in edges)) + 1)
        this = [0.0]*(w*h)
        for py in range(ymin, ymax+1):
            for s in range(sub):
                sy = py + (s + 0.5)/sub
                xs = []
                for (y0, y1, x0, x1) in edges:
                    if (y0 <= sy < y1) or (y1 <= sy < y0):
                        xs.append(x0 + (sy - y0)*(x1 - x0)/(y1 - y0))
                if not xs:
                    continue
                xs.sort()
                for i in range(0, len(xs)-1, 2):
                    xa, xb = xs[i], xs[i+1]
                    if xb <= 0 or xa >= w:
                        continue
                    xa = max(xa, 0.0); xb = min(xb, float(w))
                    ia, ib = int(xa), int(xb)
                    row = py*w
                    if ia == ib:
                        this[row+ia] += (xb-xa)/sub
                    else:
                        this[row+ia] += (ia+1-xa)/sub
                        for px in range(ia+1, ib):
                            this[row+px] += 1.0/sub
                        if ib < w:
                            this[row+ib] += (xb-ib)/sub
        for i in range(w*h):
            v = this[i]
            if v > cov[i]:
                cov[i] = 1.0 if v > 1.0 else v
    return cov

def over(dst, cov, rgb, w, h):
    r, g, b = rgb
    for i in range(w*h):
        a = cov[i]
        if a <= 0:
            continue
        da = dst[i*4+3]/255.0
        na = a + da*(1-a)
        if na <= 0:
            continue
        for c, sc in enumerate((r, g, b)):
            dc = dst[i*4+c]
            dst[i*4+c] = int(round((sc*a + dc*da*(1-a))/na))
        dst[i*4+3] = int(round(na*255))

def write_png(path, buf, w, h):
    raw = b''.join(b'\x00' + bytes(buf[y*w*4:(y+1)*w*4]) for y in range(h))
    def chunk(tag, data):
        c = struct.pack('>I', len(data)) + tag + data
        return c + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff)
    png = (b'\x89PNG\r\n\x1a\n'
           + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
           + chunk(b'IDAT', zlib.compress(raw, 9))
           + chunk(b'IEND', b''))
    open(path, 'wb').write(png)

def render(S):
    k = S/1024.0
    def sc(pts): return [(x*k, y*k) for (x, y) in pts]
    buf = [0]*(S*S*4)

    # The ground: a Big Sur squircle-proportioned rounded square, inset so the
    # Dock's own spacing is respected.
    ground = sc(round_rect(100, 100, 924, 924, 185))
    over(buf, coverage([ground], S, S), PAPER, S, S)

    # A hairline edge, so a paper-coloured icon still has a shape on a white
    # background.
    outer = coverage([ground], S, S)
    inner = coverage([sc(round_rect(100+7, 100+7, 924-7, 924-7, 178))], S, S)
    ring = [max(0.0, outer[i] - inner[i]) for i in range(S*S)]
    over(buf, ring, EDGE, S, S)

    # The perch: one quiet rule, the same line a feed row sits on. Weighted so
    # it is still a line at 32 px rather than the pale smudge it was.
    over(buf, coverage([sc(round_rect(246, 702, 794, 728, 13))], S, S), PERCHGREY, S, S)

    # The bird, as one silhouette: body, head, tail, beak and legs unioned so
    # no seam shows where they meet. Sized to carry the tile, because a smaller
    # bird stopped being a bird somewhere under 32 px.
    bird = [
        sc(ellipse(504, 544, 138, 121, -8)),
        sc(circle(583, 450, 93)),
        sc([(661, 436), (750, 463), (661, 490)]),          # beak
        sc([(283, 400), (407, 465), (391, 588)]),          # tail, upswept
        sc(round_rect(486, 646, 508, 714, 11)),            # legs
        sc(round_rect(555, 646, 577, 714, 11)),
    ]
    over(buf, coverage(bird, S, S), TERRA, S, S)

    # The eye, punched back out in the paper colour.
    over(buf, coverage([sc(circle(613, 430, 17))], S, S), PAPER, S, S)
    return buf

if __name__ == '__main__':
    import sys
    S = int(sys.argv[1]); out = sys.argv[2]
    write_png(out, render(S), S, S)
    print(f"{out} {S}x{S}")
