"""LXGW Neo XiHei Screen -> KinNovel 补充字体: 只留 Kindle 系统黑体 (STHeitiMedium) 缺的符号、标点、拉丁/希腊/西里尔扩展。

用法: python tools/gen_symbol_font.py <LXGWNeoXiHeiScreen*.ttf> <STHeitiMedium.ttf> <NotoEmoji-subset.ttf> <out.ttf>
字体来源 https://github.com/lxgw/LxgwNeoXiHei (IPA Font License 1.0)。按许可证第 3 条, 派生字体改名 (KinNovel Symbols)。
"""
import sys
from fontTools.ttLib import TTFont
from fontTools import subset

src, heiti, emoji, dst = sys.argv[1:5]
cmap = TTFont(src, lazy=True).getBestCmap()
have = set(TTFont(heiti, lazy=True).getBestCmap()) | set(TTFont(emoji, lazy=True).getBestCmap())
RANGES = [
    (0x00A0, 0x05FF),  # 拉丁补充/扩展、IPA、希腊、西里尔
    (0x1D00, 0x1FFF),  # 语音扩展、拉丁扩展附加、希腊扩展
    (0x2000, 0x2BFF),  # 标点、字母式符号、箭头、数学、技术、制表、几何、杂项符号 (♡♢♤♧)、装饰 (✿✦)
    (0x2E00, 0x2E7F),  # 补充标点
    (0x3000, 0x303F),  # CJK 标点 (〜)
    (0x3200, 0x33FF),  # 带圈 CJK、CJK 兼容
    (0xFE10, 0xFE6F),  # 竖排/小写/全角变体
    (0xFF00, 0xFFEF),  # 全角/半角
]
keep = sorted(cp for cp in cmap if cp not in have and any(a <= cp <= b for a, b in RANGES))
f = TTFont(src)
opts = subset.Options()
opts.layout_features = []
opts.hinting = False
opts.drop_tables += ["GSUB", "GPOS", "GDEF", "STAT", "DSIG", "vhea", "vmtx", "VORG"]
opts.name_IDs = [0, 13, 14]
opts.notdef_outline = False
s = subset.Subsetter(opts)
s.populate(unicodes=keep)
s.subset(f)
name = f["name"]
for nid, text in ((1, "KinNovel Symbols"), (2, "Regular"), (3, "KinNovel Symbols"), (4, "KinNovel Symbols"), (6, "KinNovelSymbols")):
    name.setName(text, nid, 3, 1, 0x409)
f.save(dst)
print(len(keep), "code points")
