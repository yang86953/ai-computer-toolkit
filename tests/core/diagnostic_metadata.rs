#![cfg(target_os = "windows")]

// 导入 Rust 运行诊断入口。
use ai_computer_toolkit::AppControlService;
// 导入通用 JSON 值。
use serde_json::Value;

// 递归检查运行诊断中是否出现迁移元数据字段。
fn assert_forbidden_fields_absent(
    // 接收当前 JSON 节点。
    value: &Value,
    // 接收封闭禁止字段集合。
    forbidden: &[&str],
) {
    // 按 JSON 节点类型递归。
    match value {
        // 检查对象键和所有子值。
        Value::Object(object) => {
            // 遍历对象字段。
            for (key, child) in object {
                // 运行诊断不得携带构建或 C++ 迁移字段。
                assert!(!forbidden.contains(&key.as_str()), "forbidden field: {key}");
                // 递归检查子对象与数组。
                assert_forbidden_fields_absent(child, forbidden);
            }
        }
        // 检查数组中的每个元素。
        Value::Array(items) => {
            // 遍历数组元素。
            for item in items {
                // 递归检查当前元素。
                assert_forbidden_fields_absent(item, forbidden);
            }
        }
        // 标量没有字段名。
        _ => {}
    }
}

// Rust doctor/status 必须只发布运行事实，不能混入迁移元数据。
#[test]
fn runtime_diagnostics_exclude_build_and_cpp_migration_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    // 解析字段归属策略清单。
    let policy: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../contracts/runtime-diagnostic-metadata-policy-v1.json"
    ))?;
    // 解析 Rust runtime doctor envelope。
    let schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/runtime-diagnostic-envelope.schema.json"
    ))?;
    // 解析 C++ doctor 对照 schema。
    let cpp_schema: Value = serde_json::from_str(include_str!(
        // 使用编译期固定路径。
        "../../contracts/v1/doctor-result.schema.json"
    ))?;
    // 执行覆盖全部公开 status surface 的 Rust doctor。
    let result = AppControlService::new().doctor(None);
    // 取得 runtime 顶层实际字段并排序。
    let mut actual_fields = result
        // 要求 doctor 返回对象。
        .as_object()
        // 将损坏结果转为测试错误。
        .ok_or_else(|| std::io::Error::other("doctor must return an object"))?
        // 读取全部字段名。
        .keys()
        // 建立独立字符串所有权。
        .cloned()
        // 收集为可排序数组。
        .collect::<Vec<_>>();
    // 固定字段顺序用于比较。
    actual_fields.sort();
    // 取得清单要求字段并排序。
    let mut expected_fields = policy["runtimeTopLevelFields"]
        // 要求数组形状。
        .as_array()
        // 将损坏清单转为测试错误。
        .ok_or_else(|| std::io::Error::other("runtimeTopLevelFields must be an array"))?
        // 遍历 JSON 字符串。
        .iter()
        // 只接受字符串字段名。
        .map(|value| {
            // 将错误类型统一为标准 IO 错误。
            value
                // 读取字符串。
                .as_str()
                // 拒绝非字符串。
                .map(str::to_owned)
                // 返回结构化测试错误。
                .ok_or_else(|| std::io::Error::other("runtime field must be a string"))
        })
        // 收集并传播字段错误。
        .collect::<Result<Vec<_>, _>>()?;
    // 固定清单字段顺序用于比较。
    expected_fields.sort();
    // runtime 实际字段必须与策略清单完全一致。
    assert_eq!(actual_fields, expected_fields);
    // schema 顶层字段也必须与相同清单一致。
    let mut schema_fields = schema["properties"]
        // 要求 properties 对象。
        .as_object()
        // 将损坏 schema 转为测试错误。
        .ok_or_else(|| std::io::Error::other("schema properties must be an object"))?
        // 读取字段名。
        .keys()
        // 建立独立所有权。
        .cloned()
        // 收集为数组。
        .collect::<Vec<_>>();
    // 固定 schema 字段顺序。
    schema_fields.sort();
    // schema 不得与 runtime 策略漂移。
    assert_eq!(schema_fields, expected_fields);
    // 取得禁止字段集合。
    let forbidden = policy["forbiddenRuntimeFields"]
        // 要求数组形状。
        .as_array()
        // 将损坏清单转为测试错误。
        .ok_or_else(|| std::io::Error::other("forbiddenRuntimeFields must be an array"))?
        // 遍历字段值。
        .iter()
        // 只接受字符串。
        .map(Value::as_str)
        // 收集完整字符串切片数组。
        .collect::<Option<Vec<_>>>()
        // 拒绝任一非字符串值。
        .ok_or_else(|| std::io::Error::other("forbidden runtime field must be a string"))?;
    // 递归确认所有 status 结果都没有迁移元数据。
    assert_forbidden_fields_absent(&result, &forbidden);
    // C++ schema 必须明确标为只读对照基线。
    assert_eq!(
        policy["cppDoctorSchemaRole"],
        "reference-compatibility-baseline"
    );
    // C++ schema 的注释必须阻止 Rust 调用者误用。
    assert!(cpp_schema["$comment"].as_str().is_some_and(|comment| {
        // 同时要求出现 Rust runtime schema 名称。
        comment.contains("Rust runtime diagnostics")
            // 并明确该 schema 只属于 C++ compatibility。
            && comment.contains("C++ compatibility baseline only")
    }));
    // 返回字段归属一致成功。
    Ok(())
}
