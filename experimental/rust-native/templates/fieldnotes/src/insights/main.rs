#[tokio::main]
async fn main() -> Result<(), ::worker::native::RuntimeError> {
    fieldnotes_insights::bootstrapper::run().await
}
