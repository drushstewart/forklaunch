use ecommerce_sdk::*;
use std::collections::BTreeMap;
fn headers(key: &str) -> BTreeMap<String, String> { BTreeMap::from([("authorization".into(), std::env::var(key).unwrap())]) }
#[tokio::test]
async fn generated_rust_sdk_reads_typed_catalog_and_orders() {
    let client = EcommerceClient::new(std::env::var("ECOMMERCE_BASE_URL").unwrap());
    let catalog = client.product_list_catalog(EcommerceProductListCatalogRequest {
        query: BTreeMap::from([("ids".into(), "[\"product\"]".into()), ("limit".into(), "1".into())]),
        headers: headers("ECOMMERCE_CATALOG_HEADER"), ..Default::default()
    }).await.unwrap();
    let EcommerceProductListCatalogResponses::Status200(page) = catalog else { panic!("catalog failed") };
    assert_eq!(page.total, 1); assert_eq!(page.products.len(), 1);
    let order = client.order_get_order(EcommerceOrderGetOrderRequest {
        params: BTreeMap::from([("id".into(), "order".into())]), headers: headers("ECOMMERCE_ORDER_HEADER"), ..Default::default()
    }).await.unwrap();
    let EcommerceOrderGetOrderResponses::Status200(order) = order else { panic!("order failed") };
    assert_eq!(order.id, "order"); assert_eq!(order.total_cents, 200);
    let denied = client.order_get_order(EcommerceOrderGetOrderRequest {
        params: BTreeMap::from([("id".into(), "order".into())]), ..Default::default()
    }).await.unwrap();
    assert!(matches!(denied, EcommerceOrderGetOrderResponses::Error { status: 401, .. }));
}
