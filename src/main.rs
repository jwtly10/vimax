#[macro_use]
mod macros;
mod action;
mod app;
mod buffer;
mod editor;
mod layout;
mod lsp;
mod picker;
mod registers;
mod syntax;
mod text_grid;
mod ui;
mod undo;

mod vim;
mod window;
mod workspace;

use app::Remax;
use tracing::info;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("remax=debug")),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("remax starting");

    iced::application(Remax::boot, Remax::update, Remax::view)
        .subscription(Remax::subscription)
        .theme(Remax::theme)
        .exit_on_close_request(false)
        .run()
}
