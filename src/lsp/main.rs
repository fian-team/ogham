use tower_lsp::{LspService, Server};

mod document;
mod hover;
mod semantic_tokens;
mod server;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::OghamLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
