use std::collections::BTreeMap;

use ecommerce_sdk::*;
fn headers(key: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("authorization".into(), std::env::var(key).unwrap())])
}
#[tokio::test]
async fn generated_rust_sdk_reads_typed_catalog_and_orders() {
    let client = Client::new(std::env::var("ECOMMERCE_BASE_URL").unwrap());
    let catalog = client
        .list_catalog(ListCatalogRequest {
            query: serde_json::from_value(serde_json::json!({"ids":["product"], "limit":1}))
                .unwrap(),
            headers: headers("ECOMMERCE_CATALOG_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let ListCatalogResponses::Status200(page) = catalog else {
        panic!("catalog failed")
    };
    assert_eq!(page.total, 1);
    assert_eq!(page.products.len(), 1);
    let order = client
        .get_order(GetOrderRequest {
            params: GetOrderParams { id: "order".into() },
            headers: headers("ECOMMERCE_ORDER_HEADER"),
            ..Default::default()
        })
        .await
        .unwrap();
    let GetOrderResponses::Status200(order) = order else {
        panic!("order failed")
    };
    assert_eq!(order.id, "order");
    assert_eq!(order.total_cents, 200);
    let denied = client
        .get_order(GetOrderRequest {
            params: GetOrderParams { id: "order".into() },
            ..Default::default()
        })
        .await
        .unwrap_err();
    // The HTTP probe checks the exact 401; this route declares no 401 response.
    assert!(matches!(
        denied,
        SdkError::Contract("undeclared response status or content type")
    ));
}
