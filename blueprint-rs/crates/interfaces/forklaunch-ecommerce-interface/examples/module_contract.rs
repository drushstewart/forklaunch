//! Exports application contracts for conformance and SDK consumers.
fn main() {
    let mut registry = forklaunch_ecommerce_interface::contracts::registry();
    registry.register(forklaunch_ecommerce_interface::contracts::import_contract());
    println!("{}", registry.into_artifact().to_json());
}
