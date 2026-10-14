"""Noto Emoji (variable) -> static wght=500, no layout tables, no hinting, emoji code points only."""
import sys
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer
from fontTools import subset

src, dst = sys.argv[1], sys.argv[2]
f = TTFont(src)
f = instancer.instantiateVariableFont(f, {"wght": 500})
cmap = f.getBestCmap()
# 跳过 ASCII / Latin-1 (界面字体都有), 但保留 © ®
keep = [cp for cp in cmap if cp >= 0x2000 or cp in (0xA9, 0xAE)]
opts = subset.Options()
opts.layout_features = []
opts.hinting = False
opts.drop_tables += ["GSUB", "GPOS", "GDEF", "STAT", "DSIG"]
opts.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
opts.notdef_outline = False
s = subset.Subsetter(opts)
s.populate(unicodes=keep)
s.subset(f)
f.save(dst)
print(len(keep), "code points")
