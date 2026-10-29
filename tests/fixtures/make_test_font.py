# Regenerates tests/fixtures/MemeTestSans.ttf (a tiny, original, box-glyph font used to test font loading).
# Usage: python tests/fixtures/make_test_font.py tests/fixtures/MemeTestSans.ttf "Meme Test Sans"   (needs fonttools)
import sys
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

def build(path, family, style="Regular"):
    chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,!?'-"
    names = [".notdef", "space"] + [f"u{ord(c):04X}" for c in chars if c != " "]
    cmap = {ord(" "): "space"}
    for c in chars:
        if c != " ": cmap[ord(c)] = f"u{ord(c):04X}"
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap(cmap)
    def box():
        pen = TTGlyphPen(None)
        pen.moveTo((80, 0)); pen.lineTo((80, 700)); pen.lineTo((520, 700)); pen.lineTo((520, 0)); pen.closePath()
        return pen.glyph()
    glyphs = {}
    metrics = {}
    for n in names:
        if n == "space":
            glyphs[n] = TTGlyphPen(None).glyph(); metrics[n] = (300, 0)
        else:
            glyphs[n] = box(); metrics[n] = (600, 80)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": family, "styleName": style})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    fb.save(path)

build(sys.argv[1], sys.argv[2])
