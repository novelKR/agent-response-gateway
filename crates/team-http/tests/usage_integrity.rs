#![cfg(any(target_os = "linux", target_os = "macos"))]
use gateway_management::{Error, Id};
use gateway_team_http::{SqliteUsage, UsageReader};
use gateway_usage_contract::{
    CanonicalUsage, Profile, RecordedEvent, RecorderConfig, SCHEMA, SCHEMA_V2,
};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

#[test]
fn exact_lookup_rejects_changed_original_event_bytes_without_modifying_rows() {
    for provider in [false, true] {
        for noncanonical in [false, true] {
            let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/team-usage-integrity-tests");
            std::fs::create_dir_all(&parent).unwrap();
            let directory = tempfile::tempdir_in(parent.canonicalize().unwrap()).unwrap();
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let mut store =
                gateway_usage_recorder::Store::open_v2(directory.path(), true, true).unwrap();
            let producer = Id::new(store.producer().unwrap()).unwrap();
            let request = Id::new("request").unwrap();
            let mut value = json!({"schema":SCHEMA,"producer_id":producer.as_str(),"request_id":request.as_str(),"attempt_id":"attempt","event_id":"event-1",
                "revision":1,"kind":"attempt_finished","started_at_ms":0,"observed_at_ms":1,"provider":"synthetic","model_alias":"model","upstream_model":"model",
                "reported_model":null,"provider_request_id":null,"provider_response_id":null,"profile":Profile::ResponsesV1,"configuration_sha256":"a".repeat(64),
                "upstream":"completed","gateway":"completed","finality":"unobserved","observation_incomplete":false,"usage":CanonicalUsage::default()});
            if provider {
                value["schema"] = json!(SCHEMA_V2);
                value.as_object_mut().unwrap().remove("profile");
                value["interpretation"] = json!({"kind":"trusted_provider_plugin","protocol":"gateway-provider/v1","provider_protocol":"synthetic/v1",
                    "package_id":"synthetic","package_version":"1.0.0","package_sha256":"a".repeat(64),"executable_sha256":"b".repeat(64)});
            }
            let original: RecordedEvent = serde_json::from_value(value.clone()).unwrap();
            store
                .record(
                    &original,
                    &RecorderConfig {
                        schema: "gateway-usage-recorder-config/v1".into(),
                        destinations: vec![],
                    },
                )
                .unwrap();
            let mut reader = SqliteUsage::open(directory.path()).unwrap();
            assert_eq!(
                reader.lookup(&producer, &request).unwrap()[0]
                    .bytes()
                    .unwrap(),
                original.bytes().unwrap()
            );
            let (payload, sha) = if noncanonical {
                let payload = serde_json::to_string_pretty(&value).unwrap();
                let sha = gateway_usage_contract::digest(payload.as_bytes());
                (payload, sha)
            } else {
                if provider {
                    value["interpretation"]["package_sha256"] = json!("c".repeat(64));
                } else {
                    value["upstream_model"] = json!("changed-model");
                }
                (
                    serde_json::to_string(&value).unwrap(),
                    gateway_usage_contract::digest(&original.bytes().unwrap()),
                )
            };
            store
                .connection
                .execute(
                    "UPDATE usage_events SET payload=?1,sha256=?2",
                    rusqlite::params![payload, sha],
                )
                .unwrap();
            assert_eq!(
                reader.lookup(&producer, &request).unwrap_err(),
                Error::InvalidStore
            );
            let unchanged: (String, String) = store
                .connection
                .query_row("SELECT payload,sha256 FROM usage_events", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
            assert_eq!(unchanged, (payload, sha));
        }
    }
}
