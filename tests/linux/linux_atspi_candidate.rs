#![cfg(all(target_os = "linux", feature = "linux-atspi-candidate"))]

//! AT-SPI v2 候选只使用双私有 D-Bus 与工具自有 exporter 的端到端验收。

#[path = "../support/atspi_private_bus.rs"]
mod atspi_private_bus;

use std::{
    io::Write,
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ai_computer_toolkit::{atspi_candidate_client, domain::error_json};
use atspi_private_bus::{AtspiFixture, NodeSpec, child, count};
use jsonschema::{Draft, Registry, Resource};
use serde_json::{Value, json};

const WORKER: &str = env!("CARGO_BIN_EXE_ai-computer-toolkit-atspi-observation-worker");
const WINDOW_SCHEMA: &str = include_str!("../../contracts/v2/window-observation.schema.json");
const METADATA_SCHEMA: &str = include_str!("../../contracts/v2/window-metadata.schema.json");
const IDENTITY_SCHEMA: &str = include_str!("../../contracts/v2/window-target-identity.schema.json");
const TREE_SCHEMA: &str = include_str!("../../contracts/v2/accessibility-tree.schema.json");
const WINDOW_V1_SCHEMA: &str = include_str!("../../contracts/v1/window-observation.schema.json");
const WINDOW_V1_IDENTITY_SCHEMA: &str =
    include_str!("../../contracts/v1/window-target-identity.schema.json");
const TREE_V1_SCHEMA: &str = include_str!("../../contracts/v1/accessibility-tree.schema.json");
const ERROR_SCHEMA: &str = include_str!("../../contracts/v1/error-envelope.schema.json");

fn worker_request(fixture: &AtspiFixture, operation: &str) -> Value {
    json!({
        "protocolVersion": "act/internal/linux-atspi-observation/v1",
        "operation": operation,
        "sessionBusAddress": fixture.session_bus.address,
        "expectedAccessibilityBusAddress": fixture.accessibility_bus.address,
        "maximumItems": 64,
        "maximumDepth": 8,
        "timeoutMs": 3000,
    })
}

fn run_worker(request: &Value) -> Result<(Output, Value), Box<dyn std::error::Error>> {
    let mut child = Command::new(WORKER)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            "unix:path=/fixture-poison-session",
        )
        .env(
            "AT_SPI_BUS_ADDRESS",
            "unix:path=/fixture-poison-accessibility",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("worker stdin missing")?
        .write_all(serde_json::to_string(request)?.as_bytes())?;
    let output = child.wait_with_output()?;
    let value = serde_json::from_slice(&output.stdout)?;
    Ok((output, value))
}

fn assert_valid(schema: &str, instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::from_str::<Value>(schema)?;
    jsonschema::draft202012::validate(&schema, instance)
        .map_err(|error| format!("schema validation failed: {error}").into())
}

fn assert_valid_window(instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let window = serde_json::from_str::<Value>(WINDOW_SCHEMA)?;
    let identity = serde_json::from_str::<Value>(IDENTITY_SCHEMA)?;
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(
            "https://local.ai-computer-toolkit/contracts/v2/window-target-identity.schema.json",
            Resource::from_contents(identity),
        )?
        .prepare()?;
    let validator = jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&window)?;
    validator
        .validate(instance)
        .map_err(|error| format!("window schema validation failed: {error}").into())
}

fn assert_valid_metadata(instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = serde_json::from_str::<Value>(METADATA_SCHEMA)?;
    let window = serde_json::from_str::<Value>(WINDOW_SCHEMA)?;
    let identity = serde_json::from_str::<Value>(IDENTITY_SCHEMA)?;
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(
            "https://local.ai-computer-toolkit/contracts/v2/window-target-identity.schema.json",
            Resource::from_contents(identity),
        )?
        .add(
            "https://local.ai-computer-toolkit/contracts/v2/window-observation.schema.json",
            Resource::from_contents(window),
        )?
        .prepare()?;
    let validator = jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&metadata)?;
    validator
        .validate(instance)
        .map_err(|error| format!("metadata schema validation failed: {error}").into())
}

fn assert_window_v1_rejects(instance: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let window = serde_json::from_str::<Value>(WINDOW_V1_SCHEMA)?;
    let identity = serde_json::from_str::<Value>(WINDOW_V1_IDENTITY_SCHEMA)?;
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(
            "https://local.ai-computer-toolkit/contracts/v1/window-target-identity.schema.json",
            Resource::from_contents(identity),
        )?
        .prepare()?;
    let validator = jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&window)?;
    assert!(!validator.is_valid(instance));
    Ok(())
}

fn assert_private_output(value: &Value, fixture: &AtspiFixture) {
    let serialized = serde_json::to_string(value).expect("serialize output");
    for forbidden in [
        fixture.session_bus.address.as_str(),
        fixture.accessibility_bus.address.as_str(),
        fixture.exporter_name.as_str(),
        "/org/a11y/",
        "fixture-poison-session",
        "fixture-poison-accessibility",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "private output leaked native material"
        );
    }
    for forbidden_key in [
        "busAddress",
        "busGuid",
        "busName",
        "objectPath",
        "processId",
        "pid",
        "uid",
        "applicationId",
        "accessibleId",
        "toolkit",
        "locale",
        "description",
        "attributes",
        "relations",
        "interfaces",
        "bounds",
        "coordinates",
        "foregroundUnchanged",
    ] {
        assert!(!serialized.contains(&format!("\"{forbidden_key}\"")));
    }
}

async fn happy_fixture() -> Result<AtspiFixture, Box<dyn std::error::Error>> {
    let mut fixture = AtspiFixture::new().await?;
    let mut button = NodeSpec::new("/org/fixture/frame/button", 43, "Confirm");
    button.enabled = true;
    fixture.add_node(button, false).await?;
    let text = NodeSpec::new("/org/fixture/frame/text", 61, "Status");
    fixture.add_node(text, false).await?;
    let frame = NodeSpec::new("/org/fixture/frame", 23, "Public Frame");
    frame.children.lock().expect("frame children").extend([
        child(fixture.exporter_name.as_str(), "/org/fixture/frame/button"),
        child(fixture.exporter_name.as_str(), "/org/fixture/frame/text"),
    ]);
    fixture.add_node(frame, true).await?;
    fixture
        .add_node(
            NodeSpec::new("/org/fixture/dialog", 16, "Public Dialog"),
            true,
        )
        .await?;
    fixture
        .add_node(
            NodeSpec::new("/org/fixture/window", 69, "Public Window"),
            true,
        )
        .await?;
    fixture
        .add_node(
            NodeSpec::new("/org/fixture/button_top", 43, "Not a window"),
            true,
        )
        .await?;
    let mut hidden = NodeSpec::new("/org/fixture/hidden", 23, "Hidden Frame");
    hidden.showing = false;
    fixture.add_node(hidden, true).await?;
    Ok(fixture)
}

#[test]
fn private_two_bus_happy_path_validates_v2_and_rejects_v1() -> Result<(), Box<dyn std::error::Error>>
{
    async_io::block_on(async {
        let fixture = happy_fixture().await?;
        let (output, discovery) = run_worker(&worker_request(&fixture, "discover"))?;
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(discovery["count"], 3);
        assert_eq!(discovery["coverage"], "partial-accessibility-exporters");
        assert_eq!(
            discovery["visibilityEvidence"],
            "accessibility-visible-and-showing"
        );
        assert_valid_window(&discovery)?;
        assert_window_v1_rejects(&discovery)?;
        assert_private_output(&discovery, &fixture);

        let target = discovery["windows"][0]["sessionId"]
            .as_str()
            .ok_or("target missing")?;
        let mut metadata_request = worker_request(&fixture, "metadata");
        metadata_request["targetId"] = json!(target);
        let (output, metadata) = run_worker(&metadata_request)?;
        assert!(output.status.success());
        assert_valid_metadata(&metadata)?;
        assert_eq!(
            metadata["window"]["targetIdentity"]["mutationAllowed"],
            false
        );
        assert_eq!(
            metadata["window"]["targetIdentity"]["sameOwnerObjectPathReuse"],
            "not-guaranteed"
        );
        assert_private_output(&metadata, &fixture);

        let mut tree_request = worker_request(&fixture, "tree");
        tree_request["targetId"] = json!(target);
        let (output, tree) = run_worker(&tree_request)?;
        assert!(output.status.success());
        assert_valid(TREE_SCHEMA, &tree)?;
        assert!(
            jsonschema::draft202012::validate(&serde_json::from_str(TREE_V1_SCHEMA)?, &tree)
                .is_err()
        );
        assert_eq!(tree["nodes"].as_array().map(Vec::len), Some(3));
        assert_eq!(tree["safety"]["providerCache"], "disabled");
        assert!(
            tree["safety"]["forbiddenApiCalls"]
                .as_object()
                .is_some_and(|calls| { calls.values().all(|count| count == 0) })
        );
        assert_private_output(&tree, &fixture);
        assert!(count(&fixture.metrics.name) > 0);
        assert!(count(&fixture.metrics.child_count) > 0);
        assert!(count(&fixture.metrics.child_at) > 0);
        Ok(())
    })
}

#[test]
fn empty_partial_and_resolution_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let fixture = AtspiFixture::new().await?;
        let (_, empty) = run_worker(&worker_request(&fixture, "discover"))?;
        assert_eq!(empty["count"], 0);
        assert_eq!(empty["truncated"], false);
        assert_valid_window(&empty)?;

        fixture.add_registry_reference(
            "org.ai_computer_toolkit.MissingExporter",
            "/org/fixture/missing",
        );
        let (_, partial) = run_worker(&worker_request(&fixture, "discover"))?;
        assert_eq!(partial["count"], 0);
        assert_eq!(partial["truncated"], true);
        assert_eq!(partial["truncationReasons"], json!(["stale-descendant"]));

        let mut stale_request = worker_request(&fixture, "metadata");
        stale_request["targetId"] = json!("s2:w:0000000000000000");
        let (output, stale) = run_worker(&stale_request)?;
        assert!(!output.status.success());
        assert_eq!(stale["error"]["code"], "STALE_SESSION");
        assert_valid(ERROR_SCHEMA, &stale)?;
        assert_private_output(&stale, &fixture);
        Ok(())
    })
}

#[test]
fn owner_generation_ambiguity_and_same_owner_reuse_stopline()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let mut fixture = AtspiFixture::new().await?;
        let node = NodeSpec::new("/org/fixture/reused", 23, "Generation One");
        let mutable_name = node.name.clone();
        fixture.add_node(node, true).await?;
        let (_, first) = run_worker(&worker_request(&fixture, "discover"))?;
        let old_target = first["windows"][0]["sessionId"]
            .as_str()
            .ok_or("target missing")?
            .to_owned();

        *mutable_name.lock().expect("name lock") = "Same Owner Replacement".to_owned();
        let mut metadata_request = worker_request(&fixture, "metadata");
        metadata_request["targetId"] = json!(old_target);
        let (_, reused) = run_worker(&metadata_request)?;
        assert_eq!(reused["window"]["title"], "Same Owner Replacement");
        assert_eq!(
            reused["window"]["targetIdentity"]["sameOwnerObjectPathReuse"],
            "not-guaranteed"
        );
        assert_eq!(
            reused["window"]["targetIdentity"]["generationOwner"],
            "none"
        );
        assert_eq!(reused["window"]["targetIdentity"]["mutationAllowed"], false);

        fixture.stop_registry().await;
        let (_, registry_down) = run_worker(&metadata_request)?;
        assert_eq!(registry_down["error"]["code"], "STALE_SESSION");
        fixture.restart_registry().await?;
        let (_, registry_recovered) = run_worker(&metadata_request)?;
        assert_eq!(registry_recovered["window"]["sessionId"], old_target);

        fixture.duplicate_top_reference("/org/fixture/reused");
        let (_, duplicated) = run_worker(&worker_request(&fixture, "discover"))?;
        let duplicate_target = duplicated["windows"][0]["sessionId"]
            .as_str()
            .ok_or("duplicate target missing")?;
        let mut ambiguous_request = worker_request(&fixture, "metadata");
        ambiguous_request["targetId"] = json!(duplicate_target);
        let (_, ambiguous) = run_worker(&ambiguous_request)?;
        assert_eq!(ambiguous["error"]["code"], "AMBIGUOUS_TARGET");

        fixture.registry_children_for_test_clear_duplicate();
        fixture.restart_exporter().await?;
        let (_, second) = run_worker(&worker_request(&fixture, "discover"))?;
        let new_target = second["windows"][0]["sessionId"]
            .as_str()
            .ok_or("new target missing")?;
        assert_ne!(old_target, new_target);
        metadata_request["targetId"] = json!(old_target);
        let (_, stale) = run_worker(&metadata_request)?;
        assert_eq!(stale["error"]["code"], "STALE_SESSION");
        Ok(())
    })
}

#[test]
fn bfs_limits_cycles_faults_and_long_names_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let mut fixture = AtspiFixture::new().await?;
        let mut faulty = NodeSpec::new("/org/fixture/root/faulty", 43, "faulty");
        faulty.fail_name = true;
        fixture.add_node(faulty, false).await?;
        let long = NodeSpec::new("/org/fixture/root/long", 61, &"界".repeat(200));
        fixture.add_node(long, false).await?;
        let mut huge = NodeSpec::new("/org/fixture/root/huge", 39, "Huge");
        huge.child_count_override = Some(u32::MAX);
        fixture.add_node(huge, false).await?;
        let root = NodeSpec::new("/org/fixture/root", 23, "Root");
        root.children.lock().expect("root children").extend([
            child(fixture.exporter_name.as_str(), "/org/fixture/root/faulty"),
            child(fixture.exporter_name.as_str(), "/org/fixture/root/long"),
            child(fixture.exporter_name.as_str(), "/org/fixture/root/huge"),
            child(fixture.exporter_name.as_str(), "/org/fixture/root"),
            child(
                fixture.exporter_name.as_str(),
                "/org/fixture/root/disappeared",
            ),
        ]);
        fixture.add_node(root, true).await?;
        let (_, discovery) = run_worker(&worker_request(&fixture, "discover"))?;
        let target = discovery["windows"][0]["sessionId"]
            .as_str()
            .ok_or("target missing")?;

        let mut request = worker_request(&fixture, "tree");
        request["targetId"] = json!(target);
        request["maximumItems"] = json!(16);
        let (_, tree) = run_worker(&request)?;
        assert_valid(TREE_SCHEMA, &tree)?;
        let reasons = tree["truncationReasons"]
            .as_array()
            .ok_or("reasons missing")?;
        for expected in [
            "cycle-detected",
            "stale-descendant",
            "property-error",
            "name-limit",
            "item-limit",
        ] {
            assert!(
                reasons.iter().any(|reason| reason == expected),
                "missing {expected}"
            );
        }
        assert!(tree["nodes"].as_array().ok_or("nodes missing")?.len() <= 16);

        request["maximumDepth"] = json!(0);
        request["maximumItems"] = json!(64);
        let (_, depth_limited) = run_worker(&request)?;
        assert_eq!(depth_limited["nodes"].as_array().map(Vec::len), Some(1));
        assert!(
            depth_limited["truncationReasons"]
                .as_array()
                .is_some_and(|reasons| reasons.iter().any(|reason| reason == "depth-limit"))
        );

        request["maximumDepth"] = json!(8);
        request["maximumItems"] = json!(1);
        let (_, item_limited) = run_worker(&request)?;
        assert_eq!(item_limited["nodes"].as_array().map(Vec::len), Some(1));
        assert!(
            item_limited["truncationReasons"]
                .as_array()
                .is_some_and(|reasons| reasons.iter().any(|reason| reason == "item-limit"))
        );
        Ok(())
    })
}

#[test]
fn permission_protocol_timeout_cancel_and_worker_recovery_are_terminal()
-> Result<(), Box<dyn std::error::Error>> {
    async_io::block_on(async {
        let denied = AtspiFixture::with_broker(0, true, None).await?;
        let (_, permission) = run_worker(&worker_request(&denied, "discover"))?;
        assert_eq!(permission["error"]["code"], "PERMISSION_DENIED");
        assert_valid(ERROR_SCHEMA, &permission)?;

        let wrong_bus =
            AtspiFixture::with_broker(0, false, Some("unix:path=/not-the-private-bus".to_owned()))
                .await?;
        let (_, protocol) = run_worker(&worker_request(&wrong_bus, "discover"))?;
        assert_eq!(protocol["error"]["code"], "WORKER_PROTOCOL_ERROR");

        let delayed = AtspiFixture::with_broker(250, false, None).await?;
        let mut timeout_request = worker_request(&delayed, "discover");
        timeout_request["timeoutMs"] = json!(25);
        let (_, timeout) = run_worker(&timeout_request)?;
        assert_eq!(timeout["error"]["code"], "TIMEOUT");
        assert_eq!(timeout["error"]["details"]["partialResultPublished"], false);
        assert_private_output(&timeout, &delayed);

        let registry_delayed = AtspiFixture::with_registry_delay(250).await?;
        let mut registry_timeout_request = worker_request(&registry_delayed, "discover");
        registry_timeout_request["timeoutMs"] = json!(25);
        let (_, registry_timeout) = run_worker(&registry_timeout_request)?;
        assert_eq!(registry_timeout["error"]["code"], "TIMEOUT");

        let mut accessible_delayed = AtspiFixture::new().await?;
        let node = NodeSpec::new("/org/fixture/delayed", 23, "Delayed Window");
        let node_delay = node.delay_ms.clone();
        accessible_delayed.add_node(node, true).await?;
        let (_, accessible_discovery) =
            run_worker(&worker_request(&accessible_delayed, "discover"))?;
        let accessible_target = accessible_discovery["windows"][0]["sessionId"]
            .as_str()
            .ok_or("delayed target missing")?;
        node_delay.store(250, std::sync::atomic::Ordering::Relaxed);
        let mut accessible_timeout_request = worker_request(&accessible_delayed, "metadata");
        accessible_timeout_request["targetId"] = json!(accessible_target);
        accessible_timeout_request["timeoutMs"] = json!(25);
        let (_, accessible_timeout) = run_worker(&accessible_timeout_request)?;
        assert_eq!(accessible_timeout["error"]["code"], "TIMEOUT");

        let mut descendant_delayed = AtspiFixture::new().await?;
        let child_node = NodeSpec::new("/org/fixture/tree/slow", 43, "Slow Child");
        child_node
            .delay_ms
            .store(250, std::sync::atomic::Ordering::Relaxed);
        descendant_delayed.add_node(child_node, false).await?;
        let root_node = NodeSpec::new("/org/fixture/tree", 23, "Tree Root");
        root_node
            .children
            .lock()
            .expect("tree children")
            .push(child(
                descendant_delayed.exporter_name.as_str(),
                "/org/fixture/tree/slow",
            ));
        descendant_delayed.add_node(root_node, true).await?;
        let (_, descendant_discovery) =
            run_worker(&worker_request(&descendant_delayed, "discover"))?;
        let descendant_target = descendant_discovery["windows"][0]["sessionId"]
            .as_str()
            .ok_or("tree target missing")?;
        let mut descendant_timeout_request = worker_request(&descendant_delayed, "tree");
        descendant_timeout_request["targetId"] = json!(descendant_target);
        descendant_timeout_request["timeoutMs"] = json!(25);
        let (_, descendant_timeout) = run_worker(&descendant_timeout_request)?;
        assert_eq!(descendant_timeout["error"]["code"], "TIMEOUT");

        let mut bus_down = happy_fixture().await?;
        let (_, before_restart) = run_worker(&worker_request(&bus_down, "discover"))?;
        let old_target = before_restart["windows"][0]["sessionId"]
            .as_str()
            .ok_or("old bus target missing")?
            .to_owned();
        bus_down.accessibility_bus.stop();
        let (_, unavailable) = run_worker(&worker_request(&bus_down, "discover"))?;
        assert_eq!(unavailable["error"]["code"], "ACCESSIBILITY_UNAVAILABLE");
        let restarted_bus = happy_fixture().await?;
        let mut old_target_request = worker_request(&restarted_bus, "metadata");
        old_target_request["targetId"] = json!(old_target);
        let (_, bus_stale) = run_worker(&old_target_request)?;
        assert_eq!(bus_stale["error"]["code"], "STALE_SESSION");

        let mut child = Command::new(WORKER)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/fixture-poison-session",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut cancel_request = worker_request(&delayed, "discover");
        cancel_request["timeoutMs"] = json!(5000);
        child
            .stdin
            .take()
            .ok_or("cancel worker stdin missing")?
            .write_all(serde_json::to_string(&cancel_request)?.as_bytes())?;
        std::thread::sleep(Duration::from_millis(30));
        child.kill()?;
        let cancelled = child.wait_with_output()?;
        assert!(!cancelled.status.success());
        assert!(
            cancelled.stdout.is_empty(),
            "cancelled worker published partial JSON"
        );

        let cancellation = Arc::new(AtomicBool::new(false));
        let trigger = cancellation.clone();
        let trigger_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            trigger.store(true, Ordering::Release);
        });
        let structured_cancel = atspi_candidate_client::run_fixture_process(
            std::path::Path::new(WORKER),
            &cancel_request,
            Duration::from_secs(2),
            || cancellation.load(Ordering::Acquire),
        )
        .expect_err("fixture launcher must map cancellation after kill+wait");
        trigger_thread
            .join()
            .expect("cancellation trigger must join");
        assert_eq!(structured_cancel.code, "CANCELLED");
        assert_eq!(structured_cancel.details["workerReaped"], true);
        assert_eq!(structured_cancel.details["partialResultPublished"], false);
        assert_valid(ERROR_SCHEMA, &error_json(&structured_cancel))?;

        let recovered = happy_fixture().await?;
        let (output, result) = run_worker(&worker_request(&recovered, "discover"))?;
        assert!(output.status.success());
        assert_eq!(result["count"], 3);
        Ok(())
    })
}
