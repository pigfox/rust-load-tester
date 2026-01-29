#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    endpoint_tester::main_entry().await
}
