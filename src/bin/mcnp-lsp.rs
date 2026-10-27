#[tokio::main]
async fn main() {
    mcnp_input_parser::lsp::run_lsp_stdio().await;
}
