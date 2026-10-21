// Current project-local client API; original legacy consumer remains unchanged.
use trees_sdk::*;

#[tokio::main]
async fn main() {
    tokio::time::timeout(std::time::Duration::from_secs(15), verify())
        .await
        .unwrap();
    println!(
        "PASS typed Rust SDK recursive request/response, optional/null, arrays and mutual records"
    );
}

async fn verify() {
    let base = std::env::args().nth(1).unwrap();
    let client = Client::new(base);
    let ReadTreeResponses::Status200(tree) = client
        .read_tree_with_options(Default::default())
        .await
        .unwrap();
    assert_eq!(tree.label, "水 🦀");
    assert_eq!(tree.children[0].label, "leaf");
    assert!(tree.parent.is_none());
    assert!(tree.optional.is_none());
    let body = serde_json::from_value(serde_json::to_value(&tree).unwrap()).unwrap();
    let WriteTreeResponses::Status200(echo) = client
        .write_tree(WriteTreeRequest {
            body: Some(body),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(echo.children[0].label, "leaf");
    let ReadForestResponses::Status200(forest) = client
        .read_forest_with_options(Default::default())
        .await
        .unwrap();
    assert!(forest[1].value[0].value.is_empty());
    let ReadChoiceResponses::Status200(choice) = client
        .read_choice_with_options(Default::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(choice).unwrap(),
        serde_json::json!({"nested":["leaf",{}]})
    );
    let ReadMutualResponses::Status200(mutual) = client
        .read_mutual_with_options(Default::default())
        .await
        .unwrap();
    assert_eq!(mutual.peer.children[0].id, "inner");
}
