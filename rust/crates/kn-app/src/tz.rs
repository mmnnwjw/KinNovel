//! 本地时区: 解析 `/etc/localtime` (Kindle 上指向 `/var/local/system/tz`, TZif v2)。
//! 只取 "当前时刻所处的 UTC 偏移", 足够画状态栏时钟。`KN_UTC_OFFSET_MIN` 可覆盖 (主机预览/调试)。

use std::sync::OnceLock;

/// TZif 里的转换表: (转换时刻 UTC 秒, 之后的偏移秒), 以及没有转换时的默认偏移。
#[derive(Debug, Default, PartialEq)]
struct Zone {
    transitions: Vec<(i64, i32)>,
    default_offset: i32,
}

fn be_i32(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be_i64(b: &[u8]) -> i64 {
    i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// 解析 TZif (v1 用 32 位时间; v2+ 跳过 v1 数据块, 读 64 位那一段)。
fn parse(data: &[u8]) -> Option<Zone> {
    if data.len() < 44 || &data[..4] != b"TZif" {
        return None;
    }
    let version = data[4];
    let counts = |h: &[u8]| -> [usize; 6] {
        let c = |i: usize| be_i32(&h[20 + 4 * i..]) as usize;
        // isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
        [c(0), c(1), c(2), c(3), c(4), c(5)]
    };
    let [isut, isstd, leap, time, typ, chars] = counts(data);
    let (header, time_size) = if version >= b'2' {
        let v1_len = time * 4 + time + typ * 6 + chars + leap * 8 + isstd + isut;
        let start = 44 + v1_len;
        (start, 8usize)
    } else {
        (0, 4usize)
    };
    let h = data.get(header..header + 44)?;
    if &h[..4] != b"TZif" {
        return None;
    }
    let [isut, isstd, leap, time, typ, chars] = counts(h);
    let _ = (isut, isstd, leap, chars);
    let body = data.get(header + 44..)?;
    let times = body.get(..time * time_size)?;
    let idx = body.get(time * time_size..time * time_size + time)?;
    let types = body.get(time * time_size + time..time * time_size + time + typ * 6)?;
    let offset_of = |t: usize| -> Option<i32> { types.get(t * 6..t * 6 + 4).map(be_i32) };
    let mut transitions = Vec::with_capacity(time);
    for i in 0..time {
        let at = if time_size == 8 { be_i64(&times[i * 8..]) } else { be_i32(&times[i * 4..]) as i64 };
        transitions.push((at, offset_of(idx[i] as usize)?));
    }
    // 第一个转换之前: 第一个非夏令时类型 (isdst 在每项第 5 字节), 没有就用类型 0
    let default_offset = (0..typ)
        .find(|&t| types.get(t * 6 + 4) == Some(&0))
        .and_then(offset_of)
        .or_else(|| offset_of(0))
        .unwrap_or(0);
    Some(Zone { transitions, default_offset })
}

impl Zone {
    fn offset_at(&self, utc: i64) -> i32 {
        match self.transitions.partition_point(|(at, _)| *at <= utc) {
            0 => self.default_offset,
            n => self.transitions[n - 1].1,
        }
    }
}

fn system_zone() -> &'static Option<Zone> {
    static ZONE: OnceLock<Option<Zone>> = OnceLock::new();
    ZONE.get_or_init(|| std::fs::read("/etc/localtime").ok().and_then(|d| parse(&d)))
}

/// 当前时刻的本地 UTC 偏移 (秒)。
pub fn local_utc_offset_secs(utc_now: i64) -> i64 {
    if let Some(min) = std::env::var("KN_UTC_OFFSET_MIN").ok().and_then(|v| v.parse::<i64>().ok()) {
        return min * 60;
    }
    system_zone().as_ref().map_or(0, |z| z.offset_at(utc_now) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼一个最小 TZif v2: v1 块为空, v2 块里 `transitions` 个转换。
    fn build(transitions: &[(i64, u8)], types: &[(i32, bool)]) -> Vec<u8> {
        fn header(out: &mut Vec<u8>, time: usize, typ: usize, chars: usize) {
            out.extend_from_slice(b"TZif2");
            out.extend_from_slice(&[0u8; 15]);
            for c in [0, 0, 0, time, typ, chars] {
                out.extend_from_slice(&(c as i32).to_be_bytes());
            }
        }
        let mut out = Vec::new();
        header(&mut out, 0, 1, 4);
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // 一个类型
        out.extend_from_slice(b"UTC\0");
        header(&mut out, transitions.len(), types.len(), 4);
        for (at, _) in transitions {
            out.extend_from_slice(&at.to_be_bytes());
        }
        for (_, t) in transitions {
            out.push(*t);
        }
        for (off, dst) in types {
            out.extend_from_slice(&off.to_be_bytes());
            out.push(*dst as u8);
            out.push(0);
        }
        out.extend_from_slice(b"XYZ\0");
        out
    }

    #[test]
    fn fixed_offset_zone() {
        let z = parse(&build(&[], &[(8 * 3600, false)])).unwrap();
        assert_eq!(z.offset_at(1_800_000_000), 8 * 3600);
    }

    #[test]
    fn dst_transitions() {
        // 标准 +1h, 1000 时进入夏令时 +2h, 2000 时退出
        let z = parse(&build(&[(1000, 1), (2000, 0)], &[(3600, false), (7200, true)])).unwrap();
        assert_eq!(z.offset_at(999), 3600);
        assert_eq!(z.offset_at(1000), 7200);
        assert_eq!(z.offset_at(1999), 7200);
        assert_eq!(z.offset_at(5000), 3600);
    }

    /// KPW5 上的真实 /etc/localtime (Asia/Shanghai 风格, 无夏令时)。
    #[test]
    fn real_kindle_zone_is_utc_plus_8() {
        let z = parse(include_bytes!("../tests/fixtures/kindle_localtime_utc8.tzif")).unwrap();
        assert_eq!(z.offset_at(1_791_600_000), 8 * 3600); // 2026-10
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(b"not a tz file at all, definitely not one"), None);
    }
}
