#include "kinnovel/core/Charset.hpp"
#include <unordered_map>
#include <algorithm>

namespace kinnovel::core {

namespace {

// Table of common Simplified <-> Traditional Chinese pairs
// A compact curated table covering frequent novel characters
const std::unordered_map<uint32_t, uint32_t> s_t2sMap = {
    {0x66F8, 0x4E66}, // 書 -> 书
    {0x8A71, 0x8BDD}, // 話 -> 话
    {0x8AAA, 0x8BF4}, // 說 -> 说
    {0x9EDE, 0x70B9}, // 點 -> 点
    {0x570B, 0x56FD}, // 國 -> 国
    {0x958B, 0x5F00}, // 開 -> 开
    {0x95DC, 0x5173}, // 關 -> 关
    {0x5F8C, 0x540E}, // 後 -> 后
    {0x500B, 0x4E2A}, // 個 -> 个
    {0x6642, 0x65F6}, // 時 -> 时
    {0x9019, 0x8FD9}, // 這 -> 这
    {0x90A3, 0x90A3}, // 那
    {0x5011, 0x4EEC}, // 們 -> 们
    {0x5C0D, 0x5BF9}, // 對 -> 对
    {0x6703, 0x4F1A}, // 會 -> 会
    {0x767C, 0x53D1}, // 發 -> 发
    {0x52D5, 0x52A8}, // 動 -> 动
    {0x982D, 0x5934}, // 頭 -> 头
    {0x9577, 0x957F}, // 長 -> 长
    {0x9580, 0x95E8}, // 門 -> 门
    {0x9762, 0x9762}, // 面 -> 面
    {0x8EAB, 0x8EAB}, // 身 -> 身
    {0x908A, 0x8FB9}, // 邊 -> 边
    {0x904E, 0x8FC7}, // 過 -> 过
    {0x7D93, 0x7ECF}, // 經 -> 经
    {0x8B8A, 0x53D8}, // 變 -> 变
    {0x9078, 0x9009}, // 選 -> 选
    {0x985E, 0x7C7B}, // 類 -> 类
    {0x9AD4, 0x4F53}, // 體 -> 体
    {0x898B, 0x89C1}, // 見 -> 见
    {0x8996, 0x89C6}, // 視 -> 视
    {0x89BA, 0x89C9}, // 覺 -> 觉
    {0x8A8D, 0x8BA4}, // 認 -> 认
    {0x8B58, 0x8BC6}, // 識 -> 识
    {0x8B93, 0x8BA9}, // 讓 -> 让
    {0x9032, 0x8FDB}, // 進 -> 进
    {0x9084, 0x8FD8}, // 還 -> 还
    {0x91AB, 0x533B}, // 醫 -> 医
    {0x9322, 0x94B1}, // 錢 -> 钱
    {0x9435, 0x94C1}, // 鐵 -> 铁
    {0x96E2, 0x79BB}, // 離 -> 离
    {0x96FB, 0x7535}, // 電 -> 电
    {0x98DB, 0x98DE}, // 飛 -> 飞
    {0x99AC, 0x9A6C}, // 馬 -> 马
    {0x9A45, 0x9A71}, // 驅 -> 驱
    {0x9F8D, 0x9F99}, // 龍 -> 龙
    {0x6A5F, 0x673A}, // 機 -> 机
    {0x7FA9, 0x4E49}, // 義 -> 义
    {0x5EE3, 0x5E7F}, // 廣 -> 广
    {0x61C9, 0x5E94}, // 應 -> 应
    {0x6B72, 0x5C81}, // 歲 -> 岁
    {0x5E36, 0x5E26}, // 帶 -> 带
    {0x5F35, 0x5F20}, // 張 -> 张
    {0x5F37, 0x5F3A}, // 強 -> 强
    {0x5F4E, 0x5F2F}, // 彎 -> 弯
    {0x611B, 0x7231}, // 愛 -> 爱
    {0x60F3, 0x60F3}, // 想
    {0x614B, 0x6001}, // 態 -> 态
    {0x6232, 0x620F}, // 戲 -> 戏
    {0x623F, 0x623F}, // 房
    {0x6301, 0x6301}, // 持
    {0x64DA, 0x636E}, // 據 -> 据
    {0x671B, 0x671B}, // 望
    {0x6771, 0x4E1C}, // 東 -> 东
    {0x696D, 0x4E1A}, // 業 -> 业
    {0x6A02, 0x4E50}, // 樂 -> 乐
    {0x6A23, 0x6837}, // 樣 -> 样
    {0x6B0A, 0x6743}, // 權 -> 权
    {0x6B61, 0x6B22}, // 歡 -> 欢
    {0x6B65, 0x6B65}, // 步
    {0x6C23, 0x6C14}, // 氣 -> 气
    {0x7121, 0x65E0}, // 無 -> 无
    {0x71B1, 0x70ED}, // 熱 -> 热
    {0x722D, 0x4E89}, // 爭 -> 争
    {0x7232, 0x4E3A}, // 為 -> 为
    {0x7368, 0x72EC}, // 獨 -> 独
    {0x7372, 0x83B7}, // 獲 -> 获
    {0x73FE, 0x73B0}, // 現 -> 现
    {0x7570, 0x5F02}, // 異 -> 异
    {0x7576, 0x5F53}, // 當 -> 当
    {0x7591, 0x7591}, // 疑
    {0x76E1, 0x5C3D}, // 盡 -> 尽
    {0x78BA, 0x786E}, // 確 -> 确
    {0x795E, 0x795E}, // 神
    {0x798F, 0x798F}, // 福
    {0x7A2E, 0x79CD}, // 種 -> 种
    {0x7A31, 0x79F0}, // 稱 -> 称
    {0x7B49, 0x7B49}, // 等
    {0x7BC0, 0x8282}, // 節 -> 节
    {0x7C21, 0x7B80}, // 簡 -> 简
    {0x7D22, 0x7D22}, // 索
    {0x7D04, 0x7EA6}, // 約 -> 约
    {0x7D1A, 0x7EA7}, // 級 -> 级
    {0x7D14, 0x7EAF}, // 純 -> 纯
    {0x7D50, 0x7ED3}, // 結 -> 结
    {0x7D66, 0x7ED9}, // 給 -> 给
    {0x7D71, 0x7EDF}, // 統 -> 统
    {0x7E7C, 0x7EE7}, // 繼 -> 继
    {0x7E8C, 0x7EED}, // 續 -> 续
    {0x7E3D, 0x603B}, // 總 -> 总
    {0x8077, 0x804C}, // 職 -> 职
    {0x807D, 0x542C}, // 聽 -> 听
    {0x8166, 0x8111}, // 腦 -> 脑
    {0x8173, 0x811A}, // 腳 -> 脚
    {0x81C9, 0x8138}, // 臉 -> 脸
    {0x8209, 0x4E3E}, // 舉 -> 举
    {0x8207, 0x4E0E}, // 與 -> 与
    {0x82B1, 0x82B1}, // 花
    {0x82E6, 0x82E6}, // 苦
    {0x82F1, 0x82F1}, // 英
    {0x8457, 0x8457}, // 著
    {0x8655, 0x5904}, // 處 -> 处
    {0x865F, 0x53F7}, // 號 -> 号
    {0x8853, 0x672F}, // 術 -> 术
    {0x88FD, 0x5236}, // 製 -> 制
    {0x8907, 0x590D}, // 複 -> 复
    {0x8A08, 0x8BA1}, // 計 -> 计
    {0x8A18, 0x8BB0}, // 記 -> 记
    {0x8A2A, 0x8BBF}, // 訪 -> 访
    {0x8A2D, 0x8BBE}, // 設 -> 设
    {0x8A31, 0x8BB8}, // 許 -> 许
    {0x8A62, 0x8BE2}, // 詢 -> 询
    {0x8A72, 0x8BE5}, // 該 -> 该
    {0x8A73, 0x8BE6}, // 詳 -> 详
    {0x8A93, 0x8A93}, // 誓
    {0x8A9E, 0x8BED}, // 語 -> 语
    {0x8AA4, 0x8BEF}, // 誤 -> 误
    {0x8ABF, 0x8C03}, // 調 -> 调
    {0x8AC7, 0x8C08}, // 談 -> 谈
    {0x8ACB, 0x8BF7}, // 請 -> 请
    {0x8AF8, 0x8BF8}, // 諸 -> 诸
    {0x8B1D, 0x8C22}, // 謝 -> 谢
    {0x8B49, 0x8BC1}, // 證 -> 证
    {0x8B58, 0x8BC6}, // 識 -> 识
    {0x8C4A, 0x4E30}, // 豐 -> 丰
    {0x8CB7, 0x4E70}, // 買 -> 买
    {0x8CE3, 0x5356}, // 賣 -> 卖
    {0x8CDE, 0x8D4F}, // 賞 -> 赏
    {0x8D77, 0x8D77}, // 起
    {0x8D8A, 0x8D8A}, // 越
    {0x8DD1, 0x8DD1}, // 跑
    {0x8DDD, 0x8DDD}, // 距
    {0x8DF3, 0x8DF3}, // 跳
    {0x8E8D, 0x8DC3}, // 躍 -> 跃
    {0x8ECD, 0x519B}, // 軍 -> 军
    {0x8ED2, 0x8F69}, // 軒 -> 轩
    {0x8F49, 0x8F6C}, // 轉 -> 转
    {0x8F2F, 0x8F91}, // 輯 -> 辑
    {0x8F38, 0x8F93}, // 輸 -> 输
    {0x8F2A, 0x8F6E}, // 輪 -> 轮
    {0x901A, 0x901A}, // 通
    {0x9023, 0x8FDE}, // 連 -> 连
    {0x904B, 0x8FD0}, // 運 -> 运
    {0x9053, 0x9053}, // 道
    {0x9054, 0x8FBE}, // 達 -> 达
    {0x9072, 0x8FDF}, // 遲 -> 迟
    {0x907F, 0x907F}, // 避
    {0x9109, 0x4E61}, // 鄉 -> 乡
    {0x91CD, 0x91CD}, // 重
    {0x91CF, 0x91CF}, // 量
    {0x9418, 0x949F}, // 鐘 -> 钟
    {0x9580, 0x95E8}, // 門 -> 门
    {0x9583, 0x95E1}, // 閃 -> 闪
    {0x9589, 0x95ED}, // 閉 -> 闭
    {0x9593, 0x95F4}, // 間 -> 间
    {0x95A3, 0x9601}, // 閣 -> 阁
    {0x95A5, 0x9600}, // 閥 -> 阀
    {0x9614, 0x9614}, // 阔
    {0x9670, 0x9634}, // 陰 -> 阴
    {0x9673, 0x9648}, // 陳 -> 陈
    {0x9678, 0x9646}, // 陸 -> 陆
    {0x967D, 0x9633}, // 陽 -> 阳
    {0x968A, 0x961F}, // 隊 -> 队
    {0x968E, 0x9636}, // 階 -> 阶
    {0x969A, 0x969C}, // 障
    {0x96A8, 0x968F}, // 隨 -> 随
    {0x96AA, 0x9669}, // 險 -> 险
    {0x96B1, 0x9690}, // 隱 -> 隐
    {0x96BB, 0x53EA}, // 隻 -> 只
    {0x96D9, 0x53CC}, // 雙 -> 双
    {0x96DC, 0x6742}, // 雜 -> 杂
    {0x96E3, 0x96BE}, // 難 -> 难
    {0x975C, 0x9759}, // 靜 -> 静
    {0x9802, 0x9876}, // 頂 -> 顶
    {0x9805, 0x9879}, // 項 -> 项
    {0x9806, 0x987A}, // 順 -> 顺
    {0x9818, 0x9886}, // 領 -> 领
    {0x982D, 0x5934}, // 頭 -> 头
    {0x983B, 0x9891}, // 頻 -> 频
    {0x984C, 0x9898}, // 題 -> 题
    {0x9854, 0x989C}, // 顏 -> 颜
    {0x9858, 0x613F}, // 願 -> 愿
    {0x9918, 0x4F59}, // 餘 -> 余
    {0x9928, 0x9986}, // 館 -> 馆
    {0x9996, 0x9996}, // 首
    {0x9999, 0x9999}, // 香
    {0x9A57, 0x9A8C}, // 驗 -> 验
    {0x9AD4, 0x4F53}, // 體 -> 体
    {0x9B54, 0x9B54}, // 魔
    {0x9B5A, 0x9C7C}, // 魚 -> 鱼
    {0x9CE5, 0x9E1F}, // 鳥 -> 鸟
    {0x9EA5, 0x9EA6}, // 麥 -> 麦
    {0x9EC3, 0x9EC4}, // 黃 -> 黄
    {0x9ED1, 0x9ED1}, // 黑
    {0x9EDE, 0x70B9}, // 點 -> 点
    {0x9F4A, 0x9F50}  // 齊 -> 齐
};

// Build inverted s2t map
std::unordered_map<uint32_t, uint32_t> initS2tMap() {
    std::unordered_map<uint32_t, uint32_t> m;
    for (const auto& [t, s] : s_t2sMap) {
        if (m.find(s) == m.end()) {
            m[s] = t;
        }
    }
    return m;
}

const std::unordered_map<uint32_t, uint32_t> s_s2tMap = initS2tMap();

} // namespace

std::vector<uint32_t> Charset::utf8ToCodepoints(const std::string& str) {
    std::vector<uint32_t> codepoints;
    const uint8_t* ptr = reinterpret_cast<const uint8_t*>(str.data());
    size_t len = str.size();
    size_t i = 0;

    while (i < len) {
        uint8_t c = ptr[i];
        if (c < 0x80) {
            codepoints.push_back(c);
            i += 1;
        } else if ((c & 0xE0) == 0xC0) {
            if (i + 1 < len) {
                uint32_t cp = ((c & 0x1F) << 6) | (ptr[i + 1] & 0x3F);
                codepoints.push_back(cp);
            }
            i += 2;
        } else if ((c & 0xF0) == 0xE0) {
            if (i + 2 < len) {
                uint32_t cp = ((c & 0x0F) << 12) | ((ptr[i + 1] & 0x3F) << 6) | (ptr[i + 2] & 0x3F);
                codepoints.push_back(cp);
            }
            i += 3;
        } else if ((c & 0xF8) == 0xF0) {
            if (i + 3 < len) {
                uint32_t cp = ((c & 0x07) << 18) | ((ptr[i + 1] & 0x3F) << 12) |
                              ((ptr[i + 2] & 0x3F) << 6) | (ptr[i + 3] & 0x3F);
                codepoints.push_back(cp);
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    return codepoints;
}

std::string Charset::codepointToUtf8(uint32_t cp) {
    std::string out;
    if (cp < 0x80) {
        out.push_back(static_cast<char>(cp));
    } else if (cp < 0x800) {
        out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
        out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else if (cp < 0x110000) {
        out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    }
    return out;
}

std::string Charset::codepointsToUtf8(const std::vector<uint32_t>& codepoints) {
    std::string out;
    out.reserve(codepoints.size() * 3);
    for (uint32_t cp : codepoints) {
        out += codepointToUtf8(cp);
    }
    return out;
}

bool Charset::isInvisible(uint32_t cp) {
    return (cp == 0x200B || // Zero-width space
            cp == 0x200C || // Zero-width non-joiner
            cp == 0x200D || // Zero-width joiner
            cp == 0xFEFF || // BOM / Zero-width no-break space
            cp == 0x00AD || // Soft hyphen
            cp == 0x2060);  // Word joiner
}

std::string Charset::cleanText(const std::string& text) {
    auto cps = utf8ToCodepoints(text);
    std::vector<uint32_t> filtered;
    filtered.reserve(cps.size());

    for (uint32_t cp : cps) {
        if (!isInvisible(cp)) {
            filtered.push_back(cp);
        }
    }
    return collapseWhitespace(codepointsToUtf8(filtered));
}

std::string Charset::collapseWhitespace(const std::string& text) {
    std::string res;
    res.reserve(text.size());

    bool inWhitespace = false;
    for (char c : text) {
        if (c == ' ' || c == '\t' || c == '\r' || c == '\f' || c == '\v') {
            if (!inWhitespace) {
                res.push_back(' ');
                inWhitespace = true;
            }
        } else {
            inWhitespace = false;
            res.push_back(c);
        }
    }

    // Trim edges
    size_t start = 0;
    while (start < res.size() && res[start] == ' ') start++;
    size_t end = res.size();
    while (end > start && res[end - 1] == ' ') end--;

    return res.substr(start, end - start);
}

std::string Charset::convertChinese(const std::string& text, const std::string& mode) {
    if (mode.empty() || (mode != "t2s" && mode != "s2t")) {
        return text;
    }

    auto cps = utf8ToCodepoints(text);
    const auto& map = (mode == "t2s") ? s_t2sMap : s_s2tMap;

    for (auto& cp : cps) {
        auto it = map.find(cp);
        if (it != map.end()) {
            cp = it->second;
        }
    }
    return codepointsToUtf8(cps);
}

} // namespace kinnovel::core
