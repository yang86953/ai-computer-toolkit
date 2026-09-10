//! 将 Unix 毫秒时间转换为固定 UTC RFC 3339 文本。

// 固定一天的毫秒数。
const MILLISECONDS_PER_DAY: u64 = 86_400_000;
// 固定一秒的毫秒数。
const MILLISECONDS_PER_SECOND: u64 = 1_000;

// 将非负 Unix 毫秒转换为四位年份 UTC 时间。
pub(crate) fn unix_milliseconds_to_rfc3339(milliseconds: u64) -> Option<String> {
    // 取得 Unix epoch 后完整天数。
    let days = milliseconds / MILLISECONDS_PER_DAY;
    // 转换到有符号宽整数供公历算法使用。
    let days = i128::from(days);
    // 使用 civil-from-days 算法偏移到公历纪元。
    let shifted = days.checked_add(719_468)?;
    // 非负 Unix 时间固定使用非负 era。
    let era = shifted / 146_097;
    // 取得当前 400 年周期内的天数。
    let day_of_era = shifted - era * 146_097;
    // 计算周期内年份并校正闰年边界。
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    // 恢复公历年份候选。
    let mut year = year_of_era + era * 400;
    // 取得当前年份内从三月开始的天数。
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    // 取得从三月开始的月份索引。
    let month_prime = (5 * day_of_year + 2) / 153;
    // 计算公历月内日。
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    // 将三月起算月份转换为一月起算。
    let month = if month_prime < 10 {
        // 三月至十二月加三。
        month_prime + 3
    } else {
        // 一月至二月回绕九。
        month_prime - 9
    };
    // 一月和二月属于下一个公历年。
    if month <= 2 {
        // 年份加一不会接近 i128 上限。
        year += 1;
    }
    // 公共 RFC 3339 shape 只接受四位年份。
    if !(0..=9_999).contains(&year) {
        // 拒绝无法按固定 schema 表达的时间。
        return None;
    }
    // 取得当天内毫秒余数。
    let day_milliseconds = milliseconds % MILLISECONDS_PER_DAY;
    // 取得完整小时。
    let hour = day_milliseconds / 3_600_000;
    // 取得当前小时内完整分钟。
    let minute = day_milliseconds % 3_600_000 / 60_000;
    // 取得当前分钟内完整秒。
    let second = day_milliseconds % 60_000 / MILLISECONDS_PER_SECOND;
    // 取得秒内毫秒。
    let millisecond = day_milliseconds % MILLISECONDS_PER_SECOND;
    // 格式化固定 UTC 文本。
    Some(format!(
        // 始终输出毫秒精度与 Z 时区。
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z"
    ))
}

// 声明纯公历转换回归测试。
#[cfg(test)]
mod tests {
    // 导入被测函数。
    use super::unix_milliseconds_to_rfc3339;

    // 验证 Unix epoch 与闰日边界。
    #[test]
    fn formats_epoch_and_leap_day_in_utc() {
        // Unix epoch 必须逐字匹配。
        assert_eq!(
            unix_milliseconds_to_rfc3339(0).as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        // 2000 年闰日必须保留公历百年规则。
        assert_eq!(
            unix_milliseconds_to_rfc3339(951_782_400_123).as_deref(),
            Some("2000-02-29T00:00:00.123Z")
        );
    }
}
