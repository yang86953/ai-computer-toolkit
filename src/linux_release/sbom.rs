//! Linux Release System 的 Cargo normal closure 与 SPDX 2.3 Component。

use std::collections::BTreeSet;

#[cfg(feature = "linux-release-tools")]
use std::{
    collections::{BTreeMap, VecDeque},
    process::Command,
};

use serde::{Deserialize, Serialize};

#[cfg(feature = "linux-release-tools")]
use super::sha256;
use super::{PACKAGE_NAME, ReleaseResult};

#[cfg(feature = "linux-release-tools")]
#[derive(Clone, Debug)]
pub(super) struct Dependency {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) license: String,
    pub(super) checksum: String,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    resolve: CargoResolve,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    checksum: Option<String>,
    source: Option<String>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoLock {
    package: Vec<LockPackage>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoNode>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoNode {
    id: String,
    deps: Vec<CargoDependency>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoDependency {
    pkg: String,
    dep_kinds: Vec<CargoDependencyKind>,
}

#[cfg(feature = "linux-release-tools")]
#[derive(Deserialize)]
struct CargoDependencyKind {
    kind: Option<String>,
}

/// 从 cargo metadata 的目标过滤结果遍历 normal 依赖闭包。
#[cfg(feature = "linux-release-tools")]
pub(super) fn normal_dependencies(target: &str) -> ReleaseResult<Vec<Dependency>> {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "metadata",
            "--frozen",
            "--format-version",
            "1",
            "--filter-platform",
            target,
            "--no-default-features",
        ])
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .map_err(|_| "cargo metadata inventory could not start".to_owned())?;
    if !output.status.success() {
        return Err("cargo metadata inventory failed".to_owned());
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|_| "cargo metadata inventory was malformed".to_owned())?;
    let lock_text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock"),
    )
    .map_err(|_| "Cargo.lock inventory is unavailable".to_owned())?;
    let lock: CargoLock =
        toml::from_str(&lock_text).map_err(|_| "Cargo.lock inventory is malformed".to_owned())?;
    let lock_checksums = lock
        .package
        .iter()
        .filter_map(|package| {
            package.checksum.as_ref().map(|checksum| {
                (
                    (
                        package.name.as_str(),
                        package.version.as_str(),
                        package.source.as_deref(),
                    ),
                    checksum.as_str(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let root = metadata
        .packages
        .iter()
        .find(|package| package.name == PACKAGE_NAME && package.source.is_none())
        .ok_or_else(|| "cargo metadata root package is missing".to_owned())?;
    let packages = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let nodes = metadata
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::from([root.id.as_str()]);
    let mut queue = VecDeque::from([root.id.as_str()]);
    while let Some(id) = queue.pop_front() {
        let node = nodes
            .get(id)
            .ok_or_else(|| "cargo metadata dependency node is missing".to_owned())?;
        for dependency in &node.deps {
            if !dependency
                .dep_kinds
                .iter()
                .any(|kind| kind.kind.as_deref().is_none_or(|value| value == "normal"))
            {
                continue;
            }
            let dependency_id = packages
                .get(dependency.pkg.as_str())
                .ok_or_else(|| "cargo metadata dependency package is missing".to_owned())?
                .id
                .as_str();
            if visited.insert(dependency_id) {
                queue.push_back(dependency_id);
            }
        }
    }
    let mut dependencies = Vec::new();
    for id in visited.into_iter().filter(|id| *id != root.id) {
        let package = packages
            .get(id)
            .ok_or_else(|| "cargo metadata dependency package is missing".to_owned())?;
        let license = normalize_license(package.license.as_deref())?;
        let checksum = package
            .checksum
            .as_deref()
            .or_else(|| {
                lock_checksums
                    .get(&(
                        package.name.as_str(),
                        package.version.as_str(),
                        package.source.as_deref(),
                    ))
                    .copied()
            })
            .ok_or_else(|| "release dependency checksum is missing".to_owned())?;
        validate_sha256(checksum)?;
        dependencies.push(Dependency {
            name: package.name.clone(),
            version: package.version.clone(),
            license,
            checksum: checksum.to_owned(),
        });
    }
    dependencies.sort_by(|left, right| {
        (&left.name, &left.version, &left.checksum).cmp(&(
            &right.name,
            &right.version,
            &right.checksum,
        ))
    });
    Ok(dependencies)
}

/// 生成不含 Cargo 展示后缀的稳定 notice。
#[cfg(feature = "linux-release-tools")]
pub(super) fn notices(target: &str, dependencies: &[Dependency]) -> Vec<u8> {
    let mut notice = format!(
        "Third-party dependency notice\nTarget: {target}\nGenerated from Cargo metadata normal dependency closure.\n\n"
    );
    for dependency in dependencies {
        notice.push_str(&dependency.name);
        notice.push(' ');
        notice.push_str(&dependency.version);
        notice.push_str(" | ");
        notice.push_str(&dependency.license);
        if dependency.license == "NOASSERTION" {
            notice.push_str(" | license-note: invalid-or-missing-SPDX-declaration");
        }
        notice.push('\n');
    }
    notice.into_bytes()
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpdxDocument {
    spdx_version: String,
    data_license: String,
    #[serde(rename = "SPDXID")]
    spdx_id: String,
    name: String,
    document_namespace: String,
    creation_info: CreationInfo,
    packages: Vec<SpdxPackage>,
    relationships: Vec<SpdxRelationship>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreationInfo {
    created: String,
    creators: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpdxPackage {
    #[serde(rename = "SPDXID")]
    spdx_id: String,
    name: String,
    version_info: String,
    download_location: String,
    files_analyzed: bool,
    license_concluded: String,
    license_declared: String,
    copyright_text: String,
    checksums: Vec<SpdxChecksum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpdxChecksum {
    algorithm: String,
    checksum_value: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpdxRelationship {
    spdx_element_id: String,
    relationship_type: String,
    related_spdx_element: String,
}

/// 生成并立即通过同一离线严格门禁复核 SPDX 2.3 文档。
#[cfg(feature = "linux-release-tools")]
pub(super) fn build(
    target: &str,
    dependencies: &[Dependency],
    main_binary: &[u8],
) -> ReleaseResult<Vec<u8>> {
    let root_id = "SPDXRef-Package-ai-computer-toolkit".to_owned();
    let mut packages = vec![SpdxPackage {
        spdx_id: root_id.clone(),
        name: PACKAGE_NAME.to_owned(),
        version_info: env!("CARGO_PKG_VERSION").to_owned(),
        download_location: "NOASSERTION".to_owned(),
        files_analyzed: false,
        license_concluded: "MIT".to_owned(),
        license_declared: "MIT".to_owned(),
        copyright_text: "NOASSERTION".to_owned(),
        checksums: vec![SpdxChecksum {
            algorithm: "SHA256".to_owned(),
            checksum_value: sha256(main_binary),
        }],
        comment: None,
    }];
    let mut relationships = vec![SpdxRelationship {
        spdx_element_id: "SPDXRef-DOCUMENT".to_owned(),
        relationship_type: "DESCRIBES".to_owned(),
        related_spdx_element: root_id.clone(),
    }];
    for (index, dependency) in dependencies.iter().enumerate() {
        let id = format!("SPDXRef-Dependency-{index}");
        packages.push(SpdxPackage {
            spdx_id: id.clone(),
            name: dependency.name.clone(),
            version_info: dependency.version.clone(),
            download_location: "NOASSERTION".to_owned(),
            files_analyzed: false,
            license_concluded: dependency.license.clone(),
            license_declared: dependency.license.clone(),
            copyright_text: "NOASSERTION".to_owned(),
            checksums: vec![SpdxChecksum {
                algorithm: "SHA256".to_owned(),
                checksum_value: dependency.checksum.clone(),
            }],
            comment: (dependency.license == "NOASSERTION").then(|| {
                "Cargo license declaration was missing or invalid SPDX; original omitted."
                    .to_owned()
            }),
        });
        relationships.push(SpdxRelationship {
            spdx_element_id: root_id.clone(),
            relationship_type: "DEPENDS_ON".to_owned(),
            related_spdx_element: id,
        });
    }
    let document = SpdxDocument {
        spdx_version: "SPDX-2.3".to_owned(),
        data_license: "CC0-1.0".to_owned(),
        spdx_id: "SPDXRef-DOCUMENT".to_owned(),
        name: format!("{PACKAGE_NAME}-{}-{target}", env!("CARGO_PKG_VERSION")),
        document_namespace: format!(
            "https://ai-computer-toolkit.local/spdx/{}/{target}",
            env!("CARGO_PKG_VERSION")
        ),
        creation_info: CreationInfo {
            created: "1970-01-01T00:00:00Z".to_owned(),
            creators: vec![format!("Tool: {PACKAGE_NAME}-linux-release")],
        },
        packages,
        relationships,
    };
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| "SPDX SBOM serialization failed".to_owned())?;
    validate(&bytes, target, dependencies.len() + 1)?;
    Ok(bytes)
}

/// 以固定 SPDX 2.3 子集、SPDX 表达式解析器和闭合关系验证完整文档。
pub(super) fn validate(bytes: &[u8], target: &str, package_count: usize) -> ReleaseResult<()> {
    let document: SpdxDocument =
        serde_json::from_slice(bytes).map_err(|_| "SPDX SBOM is malformed".to_owned())?;
    if document.spdx_version != "SPDX-2.3"
        || document.data_license != "CC0-1.0"
        || document.spdx_id != "SPDXRef-DOCUMENT"
        || document.name != format!("{PACKAGE_NAME}-{}-{target}", env!("CARGO_PKG_VERSION"))
        || document.document_namespace
            != format!(
                "https://ai-computer-toolkit.local/spdx/{}/{target}",
                env!("CARGO_PKG_VERSION")
            )
        || document.creation_info.created != "1970-01-01T00:00:00Z"
        || document.creation_info.creators != [format!("Tool: {PACKAGE_NAME}-linux-release")]
        || document.packages.len() != package_count
    {
        return Err("SPDX document policy is invalid".to_owned());
    }
    let mut identifiers = BTreeSet::new();
    for package in &document.packages {
        if !identifiers.insert(package.spdx_id.as_str())
            || package.spdx_id == "SPDXRef-DOCUMENT"
            || package.name.is_empty()
            || package.version_info.is_empty()
            || package.version_info.contains("(proc-macro)")
            || package.download_location != "NOASSERTION"
            || package.files_analyzed
            || package.copyright_text != "NOASSERTION"
            || package.checksums.len() != 1
            || package.checksums[0].algorithm != "SHA256"
            || (package.license_declared == "NOASSERTION")
                != (package.comment.as_deref()
                    == Some(
                        "Cargo license declaration was missing or invalid SPDX; original omitted.",
                    ))
        {
            return Err("SPDX package policy is invalid".to_owned());
        }
        validate_license(&package.license_declared)?;
        validate_license(&package.license_concluded)?;
        validate_sha256(&package.checksums[0].checksum_value)?;
    }
    let root_id = "SPDXRef-Package-ai-computer-toolkit";
    if !identifiers.contains(root_id) || document.relationships.len() != package_count {
        return Err("SPDX relationship cardinality is invalid".to_owned());
    }
    let mut described = 0_usize;
    let mut dependencies = BTreeSet::new();
    for relationship in &document.relationships {
        match relationship.relationship_type.as_str() {
            "DESCRIBES"
                if relationship.spdx_element_id == "SPDXRef-DOCUMENT"
                    && relationship.related_spdx_element == root_id =>
            {
                described += 1;
            }
            "DEPENDS_ON"
                if relationship.spdx_element_id == root_id
                    && identifiers.contains(relationship.related_spdx_element.as_str())
                    && relationship.related_spdx_element != root_id =>
            {
                if !dependencies.insert(relationship.related_spdx_element.as_str()) {
                    return Err("SPDX dependency relationship is duplicated".to_owned());
                }
            }
            _ => return Err("SPDX relationship policy is invalid".to_owned()),
        }
    }
    if described != 1 || dependencies.len() + 1 != package_count {
        return Err("SPDX relationship closure is incomplete".to_owned());
    }
    Ok(())
}

#[cfg(any(feature = "linux-release-tools", test))]
fn normalize_license(value: Option<&str>) -> ReleaseResult<String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok("NOASSERTION".to_owned());
    };
    if let Ok(expression) = spdx::Expression::parse(value) {
        return Ok(expression.to_string());
    }
    if value.contains('/') {
        let candidate = value
            .split('/')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" OR ");
        if let Ok(expression) = spdx::Expression::parse(&candidate) {
            return Ok(expression.to_string());
        }
    }
    Ok("NOASSERTION".to_owned())
}

fn validate_license(value: &str) -> ReleaseResult<()> {
    if value == "NOASSERTION" || spdx::Expression::parse(value).is_ok() {
        Ok(())
    } else {
        Err("SPDX license expression is invalid".to_owned())
    }
}

fn validate_sha256(value: &str) -> ReleaseResult<()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err("SPDX SHA256 checksum is invalid".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_license;

    #[test]
    fn cargo_license_expressions_are_normalized_or_closed() {
        assert_eq!(
            normalize_license(Some("MIT/Apache-2.0")).unwrap(),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            normalize_license(Some("MIT OR Apache-2.0")).unwrap(),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            normalize_license(Some("unregistered-license")).unwrap(),
            "NOASSERTION"
        );
    }

    #[cfg(feature = "linux-release-tools")]
    #[test]
    fn spdx_document_gate_rejects_invalid_license_and_open_relationships() {
        let dependency = super::Dependency {
            name: "fixture".to_owned(),
            version: "1.0.0".to_owned(),
            license: "MIT OR Apache-2.0".to_owned(),
            checksum: "1".repeat(64),
        };
        let bytes = super::build(
            "x86_64-unknown-linux-gnu",
            std::slice::from_ref(&dependency),
            b"fixture-binary",
        )
        .unwrap();
        super::validate(&bytes, "x86_64-unknown-linux-gnu", 2).unwrap();
        let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        invalid["packages"][1]["licenseDeclared"] = serde_json::json!("MIT/Apache-2.0");
        assert!(
            super::validate(
                &serde_json::to_vec(&invalid).unwrap(),
                "x86_64-unknown-linux-gnu",
                2,
            )
            .is_err()
        );
        let mut open: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        open["relationships"].as_array_mut().unwrap().pop();
        assert!(
            super::validate(
                &serde_json::to_vec(&open).unwrap(),
                "x86_64-unknown-linux-gnu",
                2,
            )
            .is_err()
        );
    }
}
