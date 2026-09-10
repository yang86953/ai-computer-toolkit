//! MCP 唯一二进制入口的无桌面副作用回归。
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};
struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Client {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }
    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    fn rpc(&mut self, id: u32, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let until = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() > until {
                let _ = self.child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.wait();
    }
}
#[test]
fn binary_discovers_one_catalog_and_refuses_unauthorized_desktop() {
    let output = Command::new(env!("CARGO_BIN_EXE_ai-computer-toolkit"))
        .arg("--list-tools")
        .output()
        .unwrap();
    assert!(output.status.success());
    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
    // 清单是封闭集合：新增或删除工具都必须在这里显式改数，避免悄悄换掉公开面。
    assert_eq!(catalog["tools"].as_array().unwrap().len(), 8);
    let mut c = Client::new();
    assert!(c.rpc(0, "tools/list", json!({})).get("error").is_some());
    assert!(c.rpc(1,"initialize",json!({"protocolVersion":"2025-03-26"}))["result"]["capabilities"]["tools"].is_object());
    c.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    assert_eq!(c.rpc(2, "tools/list", json!({}))["result"], catalog);
    let result=c.rpc(3,"tools/call",json!({"name":"computer_connect","arguments":{"confirmed":false,"foregroundConsent":true,"strictIsolation":false}}));
    assert_eq!(result["result"]["isError"], true);
    assert!(result.to_string().contains("CONSENT_REQUIRED"));
    let result = c.rpc(4, "tools/call", json!({"name":"computer_status"}));
    assert_eq!(
        serde_json::from_str::<Value>(result["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap(),
        json!({"sessions":[]})
    );
    assert!(c.rpc(5, "initialize", json!({})).get("error").is_some());
}
#[test]
fn oversized_unterminated_mcp_input_is_rejected_without_waiting_for_newline() {
    let mut c = Client::new();
    c.input
        .as_mut()
        .unwrap()
        .write_all(&vec![b'x'; 1024 * 1024 + 1])
        .unwrap();
    c.input.as_mut().unwrap().flush().unwrap();
    let mut line = String::new();
    c.output.read_line(&mut line).unwrap();
    let result: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(result["error"]["code"], -32600);
}
