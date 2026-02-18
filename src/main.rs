#[macro_use]
mod macros;
mod action;
mod app;
mod buffer;
mod completions;
mod core_actions;
mod diagnostics;
mod editor;
mod info_panel;
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

use app::Vimax;
use tracing::info;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("vimax=debug")),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("vimax starting");

    iced::application(Vimax::boot, Vimax::update, Vimax::view)
        .subscription(Vimax::subscription)
        .theme(Vimax::theme)
        .exit_on_close_request(false)
        .run()
}
