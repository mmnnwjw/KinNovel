//! 按机型的能力表, 照搬 KOReader `frontend/device/kindle/device.lua` (各 `Kindle:extend{...}` 定义)。
//! 以 FBInk 的 `deviceName` 为键 (FBInk 不区分 PW5 与 PW5 SE, 两者只差环境光传感器, 这里用不到)。
//! 不在表里的机型 (将来的新机型) 返回 None, 调用方退回运行时探测。

/// 一个机型的能力。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelCaps {
    /// KOReader `isREAGL`: 只有 Kindle 2/3/4/DX、Touch、PaperWhite (1) 没有
    pub reagl: bool,
    /// KOReader `hasFrontlight`
    pub frontlight: bool,
    /// KOReader `hasNaturalLight` (暖光 / 色温)
    pub natural_light: bool,
    /// KOReader `canTurnFrontlightOff`: false 时 lipc 的 0 档并不关灯 (PW1/PW2/Voyage/PW3),
    /// KOReader 在前面加一个 "真关灯" 档 (直接写 sysfs)
    pub fl_off_at_zero: bool,
    /// 上面那种机型的亮度 sysfs 文件 (KOReader `fl_intensity_files`)
    pub fl_sysfs: Option<&'static str>,
    /// KOReader `hasGSensor`: 有重力感应, 可能倒拿 (Oasis 系列、Scribe 系列)
    pub gsensor: bool,
    /// Voyage: 两侧 PagePress 压感键附近的触摸要忽略 (KOReader `cold_spots`), 且无框架时要手动打开压感键
    pub voyage: bool,
}

const BASE: ModelCaps = ModelCaps {
    reagl: true,
    frontlight: true,
    natural_light: false,
    fl_off_at_zero: true,
    fl_sysfs: None,
    gsensor: false,
    voyage: false,
};

const MAX77696: &str = "/sys/class/backlight/max77696-bl/brightness";

impl ModelCaps {
    /// FBInk `deviceName` → 能力; 未知机型 None。
    pub fn for_kindle(device_name: &str) -> Option<ModelCaps> {
        let caps = match device_name {
            // 无触屏老机型 (程序本身就跑不起来, 列出来只为完整)
            "2" | "DX" | "3" | "4" => ModelCaps { reagl: false, frontlight: false, ..BASE },
            "Touch" => ModelCaps { reagl: false, frontlight: false, ..BASE },
            "PaperWhite" => ModelCaps {
                reagl: false,
                fl_off_at_zero: false,
                fl_sysfs: Some("/sys/devices/system/fl_tps6116x/fl_tps6116x0/fl_intensity"),
                ..BASE
            },
            "PaperWhite 2" | "PaperWhite 3" => ModelCaps { fl_off_at_zero: false, fl_sysfs: Some(MAX77696), ..BASE },
            "Voyage" => ModelCaps { fl_off_at_zero: false, fl_sysfs: Some(MAX77696), voyage: true, ..BASE },
            "Basic" | "Basic 2" => ModelCaps { frontlight: false, ..BASE },
            "Basic 3" | "Basic 4" | "Basic 5" | "PaperWhite 4" => BASE,
            "Oasis" | "Oasis 2" => ModelCaps { gsensor: true, ..BASE },
            "Oasis 3" => ModelCaps { natural_light: true, gsensor: true, ..BASE },
            "PaperWhite 5" | "PaperWhite 6" | "ColorSoft" => ModelCaps { natural_light: true, ..BASE },
            "Scribe" | "Scribe 2" | "Scribe 3" | "Scribe ColorSoft" => ModelCaps { natural_light: true, gsensor: true, ..BASE },
            _ => return None,
        };
        Some(caps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_koreader() {
        let c = |n| ModelCaps::for_kindle(n).unwrap();
        assert!(!c("Touch").reagl && !c("PaperWhite").reagl && c("PaperWhite 2").reagl && c("Basic").reagl);
        assert!(!c("Basic 2").frontlight && c("Basic 3").frontlight);
        assert!(c("PaperWhite 5").natural_light && !c("Basic 4").natural_light && c("Scribe").natural_light);
        assert!(!c("Voyage").fl_off_at_zero && c("Voyage").fl_sysfs.is_some() && c("PaperWhite 4").fl_off_at_zero);
        assert!(c("Oasis").gsensor && !c("PaperWhite 5").gsensor);
        assert_eq!(ModelCaps::for_kindle("Kindle 99"), None);
    }
}
