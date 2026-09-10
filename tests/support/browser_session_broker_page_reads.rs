#![cfg(target_os = "windows")]

//! 验证真实 broker stdio 纵切可达 navigate、wait 与 query。

// 导入本地 HTTP 页面、同步停止与线程所有权。
use std::{
    // 导入本地 HTTP 读写。
    io::{Read, Write},
    // 导入回环监听器。
    net::TcpListener,
    // 导入同步停止标记。
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    // 导入本地服务器线程。
    thread::{self, JoinHandle},
    // 导入非阻塞轮询时长。
    time::Duration,
};

// 导入 JSON 构造宏和值类型。
use serde_json::{Value, json};

// 导入父集成测试的固定 sibling 与断言 helper。
use super::{
    // 导入 broker 进程 owner。
    BrokerProcess,
    // 导入完整 broker 操作预算。
    COMMAND_TIMEOUT_MS,
    // 导入测试目录 owner。
    TestDirectory,
    // 导入精确字段断言。
    assert_exact_keys,
    // 导入不泄漏私有事实断言。
    assert_no_private_text,
    // 导入 worker 零残留断言。
    assert_no_worker_now,
    // 导入 fixture sibling 安装器。
    install_siblings,
    // 导入 launcher 输出解析器。
    output_json,
    // 导入独立 launcher 执行器。
    run_launcher,
};

// 保存当前测试独占的回环 HTTP 页面服务器。
struct LocalPageServer {
    // 保存可提供给浏览器的固定 URL。
    url: String,
    // 保存协作停止标记。
    stop: Arc<AtomicBool>,
    // 保存唯一服务器线程 owner。
    worker: Option<JoinHandle<()>>,
}

// 为本地页面服务器提供启动与 URL 投影。
impl LocalPageServer {
    // 在随机回环端口启动固定页面。
    fn start() -> Self {
        // 绑定操作系统分配的回环端口。
        let listener = TcpListener::bind("127.0.0.1:0")
            // 本地端口必须可用。
            .expect("local fixture page should bind");
        // 读取实际绑定地址。
        let address = listener
            // 只读取 listener 本地事实。
            .local_addr()
            // 地址查询必须成功。
            .expect("local fixture page address should exist");
        // 使用非阻塞 accept 以响应协作停止。
        listener
            // 启用非阻塞模式。
            .set_nonblocking(true)
            // 测试平台必须支持非阻塞 listener。
            .expect("local fixture page should be nonblocking");
        // 创建服务器停止标记。
        let stop = Arc::new(AtomicBool::new(false));
        // 为线程复制停止标记。
        let worker_stop = Arc::clone(&stop);
        // 启动唯一回环服务器线程。
        let worker = thread::spawn(move || {
            // 在停止前持续接受页面与 favicon 请求。
            while !worker_stop.load(Ordering::Acquire) {
                // 非阻塞处理下一条连接。
                match listener.accept() {
                    // 对任意本地请求返回同一固定页面。
                    Ok((mut stream, _)) => {
                        // 读取有界请求头以让浏览器完成发送。
                        let mut request = [0_u8; 2_048];
                        // 忽略请求内容，避免建立可变路由面。
                        let _ = stream.read(&mut request);
                        // 固定页面包含可访问按钮与唯一文本。
                        let body = b"<!doctype html><html><body><button aria-label=\"Fixture Button\">fixture text</button></body></html>";
                        // 构造关闭连接的固定 HTTP 响应头。
                        let header = format!(
                            // 声明成功、HTML、长度与关闭连接。
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            // 使用固定 body 字节数。
                            body.len(),
                        );
                        // 写入完整响应头。
                        let _ = stream.write_all(header.as_bytes());
                        // 写入固定页面内容。
                        let _ = stream.write_all(body);
                        // 立即 flush 供浏览器读取。
                        let _ = stream.flush();
                    }
                    // 无连接时短暂停顿避免 busy loop。
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        // 使用固定一毫秒轮询片。
                        thread::sleep(Duration::from_millis(1));
                    }
                    // listener 异常时结束测试服务器。
                    Err(_) => break,
                }
            }
        });
        // 返回服务器 owner 与固定回环 URL。
        Self {
            // 只公开测试进程生成的 HTTP URL。
            url: format!("http://{address}/fixture"),
            // 保存停止标记。
            stop,
            // 保存线程 owner。
            worker: Some(worker),
        }
    }

    // 返回固定页面 URL。
    fn url(&self) -> &str {
        // 借用 URL。
        &self.url
    }
}

// 测试结束时协作停止回环服务器。
impl Drop for LocalPageServer {
    // 关闭 listener 轮询并 join 唯一线程。
    fn drop(&mut self) {
        // 发布停止事实。
        self.stop.store(true, Ordering::Release);
        // 取出唯一线程 owner。
        if let Some(worker) = self.worker.take() {
            // 等待服务器线程结束。
            let _ = worker.join();
        }
    }
}

// 验证 navigate、wait、query 经真实 broker/dispatcher/System/Module 完成。
#[test]
fn production_broker_routes_page_navigation_wait_and_query() {
    // 启动当前测试独占的固定本地页面。
    let page = LocalPageServer::start();
    // 创建不接触生产安装目录的 sibling 目录。
    let directory = TestDirectory::create();
    // 安装 production broker、认证 launcher 与页面协议 worker fixture。
    let (broker_image, command_image, worker_image) = install_siblings(&directory);
    // 启动真实 production broker host。
    let broker = BrokerProcess::start(&broker_image);
    // 经独立认证 launcher 执行完整页面读取纵切。
    let output = run_launcher(
        // 使用固定主程序 sibling。
        &command_image,
        // 只传固定 operation、本地 URL 与总预算。
        json!({ "operation": "page-read-roundtrip", "url": page.url(), "timeoutMs": COMMAND_TIMEOUT_MS }),
    );
    // 解析唯一安全 JSON 输出。
    let envelope = output_json(&output);
    // 三项 operation 与最终 close 必须全部成功。
    assert!(
        // 检查 launcher 退出状态。
        output.status.success(),
        // 失败时只报告公开稳定错误码。
        "page read roundtrip failed with {} at {}",
        // 读取安全 error code 或固定占位符。
        envelope
            .pointer("/error/code")
            .and_then(Value::as_str)
            .unwrap_or("UNAVAILABLE"),
        // 读取固定阶段标签或占位符。
        envelope
            .pointer("/error/details/stage")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
    );
    // 成功 envelope 只允许 ok 与 result。
    assert_exact_keys(&envelope, &["ok", "result"]);
    // 读取完整页面结果。
    let result = envelope
        // 取得 result 对象。
        .get("result")
        // 成功必须携带 result。
        .expect("page read roundtrip should return result");
    // 页面结果只允许冻结公开字段。
    assert_exact_keys(
        result,
        &[
            "pageId",
            "navigationGeneration",
            "conditionMet",
            "query",
            "closed",
        ],
    );
    // navigate 必须签发 canonical page identity。
    assert!(
        result
            .get("pageId")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("s2:bp:") && value.len() == 38)
    );
    // 首次导航代际必须为正数。
    assert!(
        result
            .get("navigationGeneration")
            .and_then(Value::as_u64)
            .is_some_and(|value| value >= 1)
    );
    // wait completed 必须固定 conditionMet=true。
    assert_eq!(
        result.get("conditionMet").and_then(Value::as_bool),
        Some(true)
    );
    // roundtrip 必须完成 session 回收。
    assert_eq!(result.get("closed").and_then(Value::as_bool), Some(true));
    // 读取 query 结果。
    let query = result
        // 取得 provider-neutral query 对象。
        .get("query")
        // query 成功必须携带结果。
        .expect("query should return data");
    // query 只允许页面、代际与三项冻结命中字段。
    assert_exact_keys(
        query,
        &[
            "pageId",
            "navigationGeneration",
            "matches",
            "matchCount",
            "truncated",
        ],
    );
    // query 必须回显本次 current page。
    assert_eq!(query.get("pageId"), result.get("pageId"));
    // query 必须回显同一导航代际。
    assert_eq!(
        query.get("navigationGeneration"),
        result.get("navigationGeneration")
    );
    // 固定页面文本必须至少产生一个命中。
    assert!(
        query
            .get("matchCount")
            .and_then(Value::as_u64)
            .is_some_and(|value| value >= 1)
    );
    // 全部输出不得泄漏 URL、pipe、worker、epoch 或 provider 私有事实。
    assert_no_private_text(&envelope);
    // close 完成后固定页面协议 worker 不得残留。
    assert_no_worker_now(&worker_image);
    // 显式停止测试拥有的 broker。
    broker.stop();
    // 所有进程退出后隔离目录必须可确定删除。
    directory.finish();
}
