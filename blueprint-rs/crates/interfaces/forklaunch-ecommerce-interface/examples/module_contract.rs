//! Exports core ecommerce module contracts for conformance and SDK consumers.
fn main() {
    println!(
        "{}",
        forklaunch_ecommerce_interface::contracts::registry()
            .into_artifact()
            .to_json()
    );
}
