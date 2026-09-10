// 导入当前 Module 私有函数。
use super::*;

// 非 canonical 目标必须在发现前拒绝。
#[test]
// 验证 native 与旧目标不能进入正式录制。
fn rejects_noncanonical_target() {
    // 调用严格目标验证。
    let error = validate_target("window:42").err();
    // 必须返回参数错误。
    assert_eq!(error.map(|value| value.code), Some("INVALID_ARGUMENT"));
}

// 成功 envelope 不得接受额外路径字段。
#[test]
// 验证严格字段计数阻止 worker 泄漏。
fn rejects_success_envelope_with_extra_field() {
    // 构造包含全部成功字段和额外路径的 envelope。
    let envelope = json!({
        // 标记成功。
        "ok": true,
        // 使用正确协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 提供有界字段。
        "bytes": 12,
        // 提供源宽度。
        "sourceWidth": 640,
        // 提供源高度。
        "sourceHeight": 360,
        // 提供编码宽度。
        "width": 640,
        // 提供编码高度。
        "height": 360,
        // 提供帧率。
        "fps": 2,
        // 提供时长。
        "durationMs": 1000,
        // 提供编码帧数。
        "encodedFrames": 2,
        // 提供捕获帧数。
        "capturedFrames": 1,
        // 提供设备分类。
        "deviceDriver": "hardware",
        // 提供关键帧数量。
        "keyframeCount": 1,
        // 声明无音频。
        "audioCaptured": false,
        // 声明无光标。
        "cursorCaptured": false,
        // 声明前景未变。
        "foregroundUnchanged": true,
        // 注入禁止字段。
        "path": "private",
    });
    // 严格解析必须失败。
    let error = parse_worker_envelope(0, &envelope).err();
    // 返回统一协议错误。
    assert_eq!(error.map(|value| value.code), Some("WORKER_PROTOCOL_ERROR"));
}

// 精确成功 envelope 必须解析为有界录制事实。
#[test]
// 验证字段计数与关键数值边界保持一致。
fn accepts_exact_success_envelope() -> AppResult<()> {
    // 构造不含路径或原生标识的精确 envelope。
    let envelope = json!({
        // 标记成功。
        "ok": true,
        // 使用正确协议版本。
        "contractVersion": WORKER_CONTRACT_VERSION,
        // 提供视频字节数。
        "bytes": 4096,
        // 提供源宽度。
        "sourceWidth": 640,
        // 提供源高度。
        "sourceHeight": 360,
        // 提供编码宽度。
        "width": 640,
        // 提供编码高度。
        "height": 360,
        // 提供帧率。
        "fps": 2,
        // 提供时长。
        "durationMs": 1000,
        // 提供编码帧数。
        "encodedFrames": 2,
        // 提供捕获帧数。
        "capturedFrames": 1,
        // 提供封闭设备分类。
        "deviceDriver": "hardware",
        // 提供关键帧数量。
        "keyframeCount": 1,
        // 声明无音频。
        "audioCaptured": false,
        // 声明无光标。
        "cursorCaptured": false,
        // 声明前景未变。
        "foregroundUnchanged": true,
    });
    // 严格解析成功事实。
    let recording = parse_worker_envelope(0, &envelope)?;
    // 核对字节数。
    assert_eq!(recording.bytes, 4096);
    // 核对关键帧数。
    assert_eq!(recording.keyframe_count, 1);
    // 返回测试成功。
    Ok(())
}
