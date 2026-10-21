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
    let client = TreesClient::new(base);
    let TreesTreesReadTreeResponses::Status200(tree) =
        client.trees_read_tree(Default::default()).await.unwrap()
    else {
        panic!("Tree missing")
    };
    assert_eq!(tree.label, "水 🦀");
    assert_eq!(tree.children[0].label, "leaf");
    assert!(tree.parent.is_none());
    assert!(tree.optional.is_none());
    let body = serde_json::from_value(serde_json::to_value(&tree).unwrap()).unwrap();
    let TreesTreesWriteTreeResponses::Status200(echo) = client
        .trees_write_tree(TreesTreesWriteTreeRequest {
            body: Some(body),
            ..Default::default()
        })
        .await
        .unwrap()
    else {
        panic!("Echo missing")
    };
    assert_eq!(echo.children[0].label, "leaf");
    let TreesForestsReadForestResponses::Status200(forest) = client
        .forests_read_forest(Default::default())
        .await
        .unwrap()
    else {
        panic!("Forest missing")
    };
    assert!(forest[1].value[0].value.is_empty());
    let TreesChoicesReadChoiceResponses::Status200(choice) = client
        .choices_read_choice(Default::default())
        .await
        .unwrap()
    else {
        panic!("Choice missing")
    };
    assert_eq!(
        serde_json::to_value(choice).unwrap(),
        serde_json::json!({"nested":["leaf",{}]})
    );
    let TreesMutualReadMutualResponses::Status200(mutual) =
        client.mutual_read_mutual(Default::default()).await.unwrap()
    else {
        panic!("Mutual missing")
    };
    assert_eq!(mutual.peer.children[0].id, "inner");
}
