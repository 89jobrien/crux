use crux_agentic::rx;
use crux_script::HandlerRegistry;
use serde_json::json;
use std::io::Write;
use tempfile::TempDir;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    rx::register(&mut r);
    r
}

#[tokio::test]
async fn rx_install_and_list() {
    let tmp = TempDir::new().unwrap();
    let registry_path = tmp.path().join("rx").join("registry.json");

    let script_path = tmp.path().join("hello.sh");
    {
        let mut f = std::fs::File::create(&script_path).unwrap();
        writeln!(f, "#!/bin/sh\necho hello").unwrap();
    }

    let reg = registry();

    let install_handler = reg.get_handler("rx::install").unwrap();
    let result = install_handler(json!({
        "args": {
            "name": "hello",
            "source": script_path.to_string_lossy(),
            "description": "says hello",
            "registry": registry_path.to_string_lossy(),
        },
    }))
    .await
    .unwrap();

    assert_eq!(result["installed"], "hello");
    assert!(result["path"].as_str().unwrap().contains("hello"));

    let list_handler = reg.get_handler("rx::list").unwrap();
    let list_result = list_handler(json!({
        "args": { "registry": registry_path.to_string_lossy() },
    }))
    .await
    .unwrap();

    let commands = list_result["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["name"], "hello");
    assert_eq!(commands[0]["description"], "says hello");

    let dest = tmp.path().join("rx").join("bin").join("hello");
    assert!(dest.exists());
    let contents = std::fs::read_to_string(&dest).unwrap();
    assert!(contents.contains("echo hello"));
}

#[tokio::test]
async fn rx_install_missing_source_fails() {
    let tmp = TempDir::new().unwrap();
    let registry_path = tmp.path().join("rx").join("registry.json");

    let reg = registry();
    let handler = reg.get_handler("rx::install").unwrap();

    let result = handler(json!({
        "args": {
            "name": "bogus",
            "source": "/nonexistent/script.sh",
            "registry": registry_path.to_string_lossy(),
        },
    }))
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn rx_install_replaces_existing_entry() {
    let tmp = TempDir::new().unwrap();
    let registry_path = tmp.path().join("rx").join("registry.json");

    let script_v1 = tmp.path().join("v1.sh");
    std::fs::write(&script_v1, "#!/bin/sh\necho v1").unwrap();
    let script_v2 = tmp.path().join("v2.sh");
    std::fs::write(&script_v2, "#!/bin/sh\necho v2").unwrap();

    let reg = registry();
    let install = reg.get_handler("rx::install").unwrap();

    let mk_input = |source: &std::path::Path| {
        json!({
            "args": {
                "name": "tool",
                "source": source.to_string_lossy(),
                "registry": registry_path.to_string_lossy(),
            },
        })
    };

    install(mk_input(&script_v1)).await.unwrap();
    install(mk_input(&script_v2)).await.unwrap();

    let list = reg.get_handler("rx::list").unwrap();
    let list_result = list(json!({
        "args": { "registry": registry_path.to_string_lossy() },
    }))
    .await
    .unwrap();
    let commands = list_result["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 1);

    let dest = tmp.path().join("rx").join("bin").join("tool");
    let contents = std::fs::read_to_string(dest).unwrap();
    assert!(contents.contains("echo v2"));
}
