//! 验证 sequence 结构化模板的确定性渲染语义。

// 导入 JSON 构造宏和值类型。
use serde_json::{Value, json};

// 导入受测模板段和渲染入口。
use super::{SequenceTemplateSegment, render_sequence_template};

// 验证 literal 与严格字符串来源按声明顺序原样拼接。
#[test]
fn renders_literal_and_string_source_in_order() -> Result<(), Box<dyn std::error::Error>> {
    // 构造一个成功且完整纳入预算的来源步骤记录。
    let results = vec![json!({
        // 标记 provider 成功。
        "ok": true,
        // 提供模板可读取的字符串字段。
        "result": { "app": "desktop" }
    })];
    // 构造无表达式语义的三个有序段。
    let segments = vec![
        // 前缀逐字输出。
        SequenceTemplateSegment::Literal {
            // 不解释连字符。
            text: "prefix-".to_owned(),
        },
        // 中间段读取第一步字符串。
        SequenceTemplateSegment::Source {
            // 使用一基来源步骤索引。
            source_step: 1,
            // 读取 app 字段。
            source_pointer: "/app".to_owned(),
        },
        // 后缀逐字输出。
        SequenceTemplateSegment::Literal {
            // 不解释连字符。
            text: "-suffix".to_owned(),
        },
    ];
    // 执行确定性模板渲染。
    let rendered = render_sequence_template(&results, &segments)
        // 把私有失败转换成测试错误。
        .map_err(|failure| std::io::Error::other(failure.reason))?;
    // 最终值必须是一个拥有型 JSON string。
    assert_eq!(
        // 读取实际渲染值。
        rendered,
        // 对比逐段原样拼接结果。
        Value::String("prefix-desktop-suffix".to_owned())
    );
    // 测试正常完成。
    Ok(())
}
