use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};

use unity_asset::environment::{BinaryObjectKey, Environment, EnvironmentObjectRef};
use unity_asset::{get_class_name_str, UnityValue};
use unity_asset_binary::asset::SerializedFileParser;
use unity_asset_binary::bundle::BundleLoadOptions;
use unity_asset_binary::file::load_bundle_file_with_options;

const DEFAULT_NEEDLES: &[&str] = &[
    "01 - Field Day",
    "Straw",
    "fdca6e12",
    "add7945e",
    "gateBigLiftoffFinish",
    "<Track",
    "<Race",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut inputs = Vec::new();
    let mut use_default_needles = true;
    let mut custom_needles = Vec::new();
    let mut dump_class_ids = BTreeSet::new();
    let mut dump_path_ids = BTreeSet::new();

    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--needle" {
            let Some(needle) = args.next() else {
                return Err("--needle requires a value".into());
            };
            custom_needles.push(needle.to_string_lossy().to_string());
        } else if arg == "--class-id" {
            let Some(class_id) = args.next() else {
                return Err("--class-id requires a value".into());
            };
            dump_class_ids.insert(class_id.to_string_lossy().parse::<i32>()?);
        } else if arg == "--path-id" {
            let Some(path_id) = args.next() else {
                return Err("--path-id requires a value".into());
            };
            dump_path_ids.insert(path_id.to_string_lossy().parse::<i64>()?);
        } else if arg == "--no-default-needles" {
            use_default_needles = false;
        } else if arg == "--help" || arg == "-h" {
            print_usage();
            return Ok(());
        } else {
            inputs.push(PathBuf::from(arg));
        }
    }

    if inputs.is_empty() {
        print_usage();
        return Err("provide at least one Unity bundle, asset file, or directory".into());
    }

    let mut needles = Vec::new();
    if use_default_needles {
        needles.extend(DEFAULT_NEEDLES.iter().map(|needle| needle.to_string()));
    }
    needles.extend(custom_needles);

    for input in &inputs {
        print_raw_file_hits(input, &needles)?;
        print_binary_externals(input)?;
    }

    let mut env = Environment::new();
    for input in &inputs {
        env.load(input)?;
    }

    println!();
    println!("environment");
    println!("  yaml_documents: {}", env.yaml_documents().len());
    println!("  binary_assets: {}", env.binary_assets().len());
    println!("  bundles: {}", env.bundles().len());
    println!("  webfiles: {}", env.webfiles().len());
    println!("  warnings: {}", env.warnings().len());
    for warning in env.warnings().iter().take(20) {
        println!("    warning: {warning}");
    }

    println!();
    println!("bundles");
    for (source, bundle) in env.bundles() {
        println!("  {}", source.describe());
        println!("    assets: {}", bundle.asset_count());
        println!("    files: {}", bundle.file_count());
        println!("    compressed: {}", bundle.is_compressed());
        print_limited(
            "asset_names",
            bundle.asset_names.iter().map(String::as_str),
            12,
        );
        print_limited("node_names", bundle.node_names().into_iter(), 12);
        print_limited("file_names", bundle.file_names().into_iter(), 12);
    }

    print_container_matches(&env, &needles);
    inspect_objects(&env, &needles)?;
    dump_class_objects(&env, &dump_class_ids)?;
    dump_path_objects(&env, &dump_path_ids)?;

    Ok(())
}

fn print_binary_externals(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_dir() {
        return Ok(());
    }

    let mut options = BundleLoadOptions::lazy();
    options.max_memory = Some(768 * 1024 * 1024);
    options.max_unityfs_block_cache_memory = Some(256 * 1024 * 1024);
    options.max_compressed_block_size = Some(768 * 1024 * 1024);

    let Ok(bundle) = load_bundle_file_with_options(path, options) else {
        return Ok(());
    };

    println!("serialized file externals: {}", path.display());
    for node in &bundle.nodes {
        if node.size > 768 * 1024 * 1024 {
            println!("  node {} skipped: size={}", node.name, node.size);
            continue;
        }
        let Ok(bytes) = bundle.extract_node_data(node) else {
            continue;
        };
        let Ok(file) = SerializedFileParser::from_bytes_with_options(bytes, false) else {
            continue;
        };
        println!(
            "  node {}: objects={} externals={}",
            node.name,
            file.objects.len(),
            file.externals.len()
        );
        for (idx, external) in file.externals.iter().enumerate().take(16) {
            println!(
                "    file_id={} type={} guid={} path={}",
                idx + 1,
                external.type_,
                external.guid_string(),
                external.path
            );
        }
    }
    Ok(())
}

fn print_usage() {
    eprintln!(
        "usage: cargo run --bin inspect_unity_asset -- <bundle-or-dir> [more paths] [--no-default-needles] [--needle text] [--class-id id] [--path-id id]"
    );
}

fn print_raw_file_hits(path: &Path, needles: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_dir() {
        println!("raw file scan: {} (directory, skipped)", path.display());
        return Ok(());
    }

    let bytes = std::fs::read(path)?;
    println!("raw file scan: {}", path.display());
    println!("  size: {}", bytes.len());
    for needle in needles {
        let hits = byte_positions(&bytes, needle.as_bytes());
        if hits.is_empty() {
            continue;
        }
        let preview = hits
            .iter()
            .take(8)
            .map(|pos| pos.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("  needle {needle:?}: {} hit(s) at {preview}", hits.len());
    }
    Ok(())
}

fn print_container_matches(env: &Environment, needles: &[String]) {
    println!();
    println!("bundle container matches");

    let mut patterns = BTreeSet::new();
    patterns.insert(String::new());
    for needle in needles {
        let normalized = needle
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == ' ' || *ch == '-' || *ch == '_')
            .collect::<String>();
        if normalized.len() >= 3 {
            patterns.insert(normalized);
        }
    }

    for pattern in patterns {
        let entries = env.find_bundle_container_entries(&pattern);
        if entries.is_empty() {
            continue;
        }
        let label = if pattern.is_empty() {
            "<all>"
        } else {
            &pattern
        };
        println!("  pattern {label:?}: {} match(es)", entries.len());
        for entry in entries.iter().take(20) {
            let key = entry
                .key
                .as_ref()
                .map(|key| format!("asset_index={:?} path_id={}", key.asset_index, key.path_id))
                .unwrap_or_else(|| "unresolved".to_string());
            println!(
                "    {} file_id={} path_id={} {key}",
                entry.asset_path, entry.file_id, entry.path_id
            );
        }
    }
}

fn dump_class_objects(
    env: &Environment,
    class_ids: &BTreeSet<i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    if class_ids.is_empty() {
        return Ok(());
    }

    println!();
    println!("class object dumps");
    let mut counts = BTreeMap::<i32, usize>::new();
    for object in env.objects() {
        let EnvironmentObjectRef::Binary(object_ref) = object else {
            continue;
        };
        let class_id = object_ref.object.class_id();
        if !class_ids.contains(&class_id) {
            continue;
        }
        let count = counts.entry(class_id).or_default();
        if *count >= 12 {
            continue;
        }
        *count += 1;

        let key = object_ref.key();
        let name = env.peek_binary_object_name(&key).ok().flatten();
        println!(
            "  class_id={} source={} asset_index={:?} path_id={} name={}",
            class_id,
            key.source.describe(),
            key.asset_index,
            key.path_id,
            name.as_deref().unwrap_or("")
        );
        match env.read_binary_object_key(&key) {
            Ok(parsed) => {
                let property_names = parsed
                    .property_names()
                    .into_iter()
                    .take(32)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "    parsed: class={} properties=[{}] warnings={}",
                    parsed.class_name(),
                    property_names,
                    parsed.typetree_warnings().len()
                );
                for (name, value) in parsed.as_unity_class().properties().iter().take(32) {
                    println!("      {name}: {}", value_summary(value, 0));
                }
                if class_id == 156 {
                    print_terrain_detail_summary(parsed.get("m_DetailDatabase"));
                }
            }
            Err(error) => {
                println!("    parse failed: {error}");
            }
        }
    }
    Ok(())
}

fn dump_path_objects(
    env: &Environment,
    path_ids: &BTreeSet<i64>,
) -> Result<(), Box<dyn std::error::Error>> {
    if path_ids.is_empty() {
        return Ok(());
    }

    println!();
    println!("path object dumps");
    let mut keys = Vec::<(BinaryObjectKey, i32)>::new();
    let mut class_ids_by_path_id = BTreeMap::<i64, i32>::new();
    for object in env.objects() {
        let EnvironmentObjectRef::Binary(object_ref) = object else {
            continue;
        };
        class_ids_by_path_id.insert(object_ref.object.path_id(), object_ref.object.class_id());
        if path_ids.contains(&object_ref.object.path_id()) {
            keys.push((object_ref.key(), object_ref.object.class_id()));
        }
    }
    keys.sort_by(|(left, _), (right, _)| {
        left.source
            .describe()
            .cmp(&right.source.describe())
            .then(left.asset_index.cmp(&right.asset_index))
            .then(left.path_id.cmp(&right.path_id))
    });

    for (key, class_id) in keys {
        let name = env.peek_binary_object_name(&key).ok().flatten();
        println!(
            "  class_id={} source={} asset_index={:?} path_id={} name={}",
            class_id,
            key.source.describe(),
            key.asset_index,
            key.path_id,
            name.as_deref().unwrap_or("")
        );
        print_parsed_object(env, &key, class_id, 96, &class_ids_by_path_id)?;
    }

    Ok(())
}

fn print_parsed_object(
    env: &Environment,
    key: &BinaryObjectKey,
    class_id: i32,
    property_limit: usize,
    class_ids_by_path_id: &BTreeMap<i64, i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    match env.read_binary_object_key(key) {
        Ok(parsed) => {
            let property_names = parsed
                .property_names()
                .into_iter()
                .take(48)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "    parsed: class={} properties=[{}] warnings={}",
                parsed.class_name(),
                property_names,
                parsed.typetree_warnings().len()
            );
            for (name, value) in parsed
                .as_unity_class()
                .properties()
                .iter()
                .take(property_limit)
            {
                println!("      {name}: {}", value_summary(value, 0));
            }
            if class_id == 156 {
                print_terrain_detail_summary(parsed.get("m_DetailDatabase"));
            }
            if class_id == 1 {
                if let Some(components) = parsed.get("m_Component").and_then(array_items) {
                    let refs = components
                        .iter()
                        .filter_map(|item| object_field(item, "component"))
                        .filter_map(|value| pptr_summary_with_class(value, class_ids_by_path_id))
                        .collect::<Vec<_>>();
                    println!("      components: {}", refs.join(", "));
                }
            }
        }
        Err(error) => {
            println!("    parse failed: {error}");
        }
    }
    Ok(())
}

fn print_terrain_detail_summary(detail_database: Option<&UnityValue>) {
    let Some(detail_database) = detail_database else {
        return;
    };
    let Some(prototypes) = object_field(detail_database, "m_DetailPrototypes").and_then(array_items)
    else {
        return;
    };
    println!("      detail prototypes: {}", prototypes.len());
    for (index, prototype) in prototypes.iter().enumerate().take(8) {
        println!("        prototype[{index}]:");
        if let UnityValue::Object(fields) = prototype {
            for (name, value) in fields {
                println!("          {name}: {}", value_summary(value, 1));
            }
        }
    }

    let Some(patches) = object_field(detail_database, "m_Patches").and_then(array_items) else {
        return;
    };
    println!("      detail patches: {}", patches.len());
    for (index, patch) in patches.iter().enumerate().take(4) {
        println!("        patch[{index}]:");
        if let UnityValue::Object(fields) = patch {
            for (name, value) in fields {
                println!("          {name}: {}", value_summary(value, 1));
            }
        }
    }
}

fn object_field<'a>(value: &'a UnityValue, key: &str) -> Option<&'a UnityValue> {
    match value {
        UnityValue::Object(fields) => fields
            .iter()
            .find(|(name, _)| name.as_str() == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn array_items(value: &UnityValue) -> Option<&[UnityValue]> {
    match value {
        UnityValue::Array(items) => Some(items),
        _ => None,
    }
}

fn pptr_summary_with_class(
    value: &UnityValue,
    class_ids_by_path_id: &BTreeMap<i64, i32>,
) -> Option<String> {
    let file_id = object_field(value, "m_FileID").and_then(UnityValue::as_i64)?;
    let path_id = object_field(value, "m_PathID").and_then(UnityValue::as_i64)?;
    let class_id = class_ids_by_path_id.get(&path_id).copied();
    Some(match class_id {
        Some(class_id) => format!("file_id={file_id} path_id={path_id} class_id={class_id}"),
        None => format!("file_id={file_id} path_id={path_id}"),
    })
}

fn value_summary(value: &UnityValue, depth: usize) -> String {
    if depth >= 2 {
        return "...".into();
    }
    match value {
        UnityValue::String(text) => format!("{text:?}"),
        UnityValue::Bytes(bytes) => format!("bytes(len={})", bytes.len()),
        UnityValue::Array(items) => {
            let parts = items
                .iter()
                .take(6)
                .map(|item| value_summary(item, depth + 1))
                .collect::<Vec<_>>()
                .join(", ");
            if items.len() > 6 {
                format!("[{parts}, ...] len={}", items.len())
            } else {
                format!("[{parts}]")
            }
        }
        UnityValue::Object(fields) => {
            let parts = fields
                .iter()
                .take(8)
                .map(|(name, item)| format!("{name}: {}", value_summary(item, depth + 1)))
                .collect::<Vec<_>>()
                .join(", ");
            if fields.len() > 8 {
                format!("{{{parts}, ...}}")
            } else {
                format!("{{{parts}}}")
            }
        }
        UnityValue::Null => "null".into(),
        UnityValue::Bool(value) => value.to_string(),
        UnityValue::Integer(value) => value.to_string(),
        UnityValue::Float(value) => value.to_string(),
    }
}

fn inspect_objects(
    env: &Environment,
    needles: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut class_counts: BTreeMap<(i32, String), usize> = BTreeMap::new();
    let mut class_ids_by_path_id = BTreeMap::<i64, i32>::new();
    let mut matched = Vec::new();

    for object in env.objects() {
        let EnvironmentObjectRef::Binary(object_ref) = object else {
            continue;
        };
        let class_id = object_ref.object.class_id();
        let key = object_ref.key();
        class_ids_by_path_id.insert(key.path_id, class_id);
        let class_name = get_class_name_str(class_id)
            .unwrap_or("Unknown")
            .to_string();
        *class_counts.entry((class_id, class_name)).or_default() += 1;

        let raw = object_ref.object.raw_data()?;
        let hits = matching_needles(raw, needles);
        if hits.is_empty() {
            continue;
        }

        let name = env.peek_binary_object_name(&key).ok().flatten();
        matched.push((key, class_id, name, hits, raw.len()));
    }

    println!();
    println!("binary object class counts");
    for ((class_id, class_name), count) in class_counts {
        println!("  {class_id:>4} {class_name:<24} {count}");
    }

    println!();
    println!("objects with raw needle hits: {}", matched.len());
    for (key, class_id, name, hits, raw_len) in matched.iter().take(40) {
        println!(
            "  source={} asset_index={:?} path_id={} class_id={} name={} raw_len={} hits={}",
            key.source.describe(),
            key.asset_index,
            key.path_id,
            class_id,
            name.as_deref().unwrap_or(""),
            raw_len,
            hits.join(", ")
        );

        match env.read_binary_object_key(key) {
            Ok(parsed) => {
                let property_names = parsed
                    .property_names()
                    .into_iter()
                    .take(24)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "    parsed: class={} properties=[{}] warnings={}",
                    parsed.class_name(),
                    property_names,
                    parsed.typetree_warnings().len()
                );
                if let Some(script) = parsed.get("m_Script").and_then(UnityValue::as_str) {
                    if let Some(summary) = text_asset_xml_summary(parsed.class_name(), script) {
                        println!("    {summary}");
                    }
                }

                let mut value_hits = Vec::new();
                for (name, value) in parsed.as_unity_class().properties() {
                    collect_value_hits(name, value, needles, &mut value_hits, 0);
                }
                for hit in value_hits.iter().take(20) {
                    println!("      value hit: {hit}");
                }
                if *class_id == 1 {
                    if let Some(active) = parsed.get("m_IsActive").and_then(UnityValue::as_bool) {
                        println!("      active: {active}");
                    }
                    if let Some(components) = parsed.get("m_Component").and_then(array_items) {
                        let refs = components
                            .iter()
                            .filter_map(|item| object_field(item, "component"))
                            .filter_map(|value| pptr_summary_with_class(value, &class_ids_by_path_id))
                            .collect::<Vec<_>>();
                        println!("      components: {}", refs.join(", "));
                    }
                }
            }
            Err(error) => {
                println!("    parse failed: {error}");
            }
        }
    }

    Ok(())
}

fn print_limited<'a>(label: &str, values: impl Iterator<Item = &'a str>, limit: usize) {
    let values = values.take(limit).collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    println!("    {label}: {}", values.join(", "));
}

fn matching_needles(bytes: &[u8], needles: &[String]) -> Vec<String> {
    needles
        .iter()
        .filter(|needle| !byte_positions(bytes, needle.as_bytes()).is_empty())
        .cloned()
        .collect()
}

fn byte_positions(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }

    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .collect()
}

fn collect_value_hits(
    path: &str,
    value: &UnityValue,
    needles: &[String],
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth > 8 || out.len() >= 200 {
        return;
    }

    match value {
        UnityValue::String(text) => {
            for needle in needles {
                if let Some(snippet) = snippet_around(text, needle, 160) {
                    out.push(format!("{path} contains {needle:?}: {snippet}"));
                }
            }
        }
        UnityValue::Bytes(bytes) => {
            for needle in matching_needles(bytes, needles) {
                out.push(format!(
                    "{path} bytes contain {needle:?} (len={})",
                    bytes.len()
                ));
            }
        }
        UnityValue::Array(items) => {
            for (index, item) in items.iter().enumerate().take(200) {
                collect_value_hits(&format!("{path}[{index}]"), item, needles, out, depth + 1);
            }
        }
        UnityValue::Object(fields) => {
            for (name, item) in fields.iter().take(200) {
                collect_value_hits(&format!("{path}.{name}"), item, needles, out, depth + 1);
            }
        }
        UnityValue::Null | UnityValue::Bool(_) | UnityValue::Integer(_) | UnityValue::Float(_) => {}
    }
}

fn snippet_around(value: &str, needle: &str, max_chars: usize) -> Option<String> {
    let byte_pos = value.find(needle)?;
    let before_chars = value[..byte_pos].chars().count();
    let needle_chars = needle.chars().count();
    let total_chars = value.chars().count();
    let padding = max_chars.saturating_sub(needle_chars) / 2;
    let start_chars = before_chars.saturating_sub(padding);
    let end_chars = (before_chars + needle_chars + padding).min(total_chars);

    let mut out = String::new();
    if start_chars > 0 {
        out.push_str("...");
    }
    out.push_str(
        &value
            .chars()
            .skip(start_chars)
            .take(end_chars.saturating_sub(start_chars))
            .collect::<String>(),
    );
    if end_chars < total_chars {
        out.push_str("...");
    }
    Some(out)
}

fn text_asset_xml_summary(class_name: &str, script: &str) -> Option<String> {
    if class_name != "TextAsset" {
        return None;
    }
    if script.contains("<Track") {
        let name = tag_text(script, "name").unwrap_or_default();
        let guid = local_id_guid(script).unwrap_or_default();
        let blueprints = chunks(script, "<TrackBlueprint", "</TrackBlueprint>").len();
        let checkpoints = chunks(script, "<TrackBlueprint", "</TrackBlueprint>")
            .into_iter()
            .filter(|chunk| chunk.contains("TrackBlueprintFlexibleCheckpoint"))
            .count();
        let gates = chunks(script, "<TrackBlueprint", "</TrackBlueprint>")
            .into_iter()
            .filter(|chunk| {
                chunk.to_ascii_lowercase().contains("gate")
                    || chunk.to_ascii_lowercase().contains("checkpoint")
                    || chunk.to_ascii_lowercase().contains("finish")
            })
            .count();
        Some(format!(
            "xml: Track name={name:?} guid={guid} blueprints={blueprints} checkpoint_blueprints={checkpoints} gate_like_blueprints={gates}"
        ))
    } else if script.contains("<Race") {
        let name = tag_text(script, "name").unwrap_or_default();
        let guid = local_id_guid(script).unwrap_or_default();
        let track_guid = first_dependency_guid(script, "TRACK").unwrap_or_default();
        let passages = chunks(
            script,
            "<RaceCheckpointPassage>",
            "</RaceCheckpointPassage>",
        )
        .len();
        Some(format!(
            "xml: Race name={name:?} guid={guid} track_guid={track_guid} passages={passages}"
        ))
    } else {
        None
    }
}

fn local_id_guid(xml: &str) -> Option<String> {
    section_text(xml, "localID").and_then(first_guid_tag)
}

fn first_dependency_guid(xml: &str, dependency_kind: &str) -> Option<String> {
    let dependency_pos = xml.find(dependency_kind)?;
    let before = &xml[..dependency_pos];
    let section_start = before.rfind("<dependency>").unwrap_or(dependency_pos);
    let after = &xml[dependency_pos..];
    let section_end = after
        .find("</dependency>")
        .map(|idx| dependency_pos + idx + "</dependency>".len())
        .unwrap_or_else(|| xml.len());
    let section = &xml[section_start..section_end];
    first_guid_tag(section)
}

fn first_guid_tag(xml: &str) -> Option<String> {
    tag_text(xml, "guid").or_else(|| tag_text(xml, "str"))
}

fn section_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start_marker = format!("<{tag}");
    let start = xml.find(&start_marker)?;
    let start_close = xml[start..].find('>')?;
    let content_start = start + start_close + 1;
    let end_marker = format!("</{tag}>");
    let end = xml[content_start..].find(&end_marker)?;
    Some(&xml[content_start..content_start + end])
}

fn tag_text(xml: &str, tag: &str) -> Option<String> {
    section_text(xml, tag).map(|text| text.trim().to_string())
}

fn chunks<'a>(text: &'a str, start_marker: &str, end_marker: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(start) = text[cursor..].find(start_marker) {
        let start = cursor + start;
        let Some(end) = text[start..].find(end_marker) else {
            break;
        };
        let end = start + end + end_marker.len();
        out.push(&text[start..end]);
        cursor = end;
    }
    out
}
