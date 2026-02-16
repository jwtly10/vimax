use std::time::{Duration, Instant};

use iced::widget::{Space, column, container, mouse_area, text};
use iced::{Alignment, Element, Length, Theme};

use crate::app::Message;

const FADE_IN_MS: f32 = 250.0;
const FADE_OUT_MS: f32 = 200.0;
const MAX_VISIBLE: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    fn bg_color(&self, alpha: f32) -> iced::Color {
        match self {
            ToastLevel::Info => iced::Color::from_rgba(0.15, 0.2, 0.3, alpha),
            ToastLevel::Success => iced::Color::from_rgba(0.15, 0.25, 0.15, alpha),
            ToastLevel::Warning => iced::Color::from_rgba(0.3, 0.25, 0.1, alpha),
            ToastLevel::Error => iced::Color::from_rgba(0.3, 0.15, 0.15, alpha),
        }
    }

    fn border_color(&self, alpha: f32) -> iced::Color {
        match self {
            ToastLevel::Info => iced::Color::from_rgba(0.4, 0.5, 0.7, alpha),
            ToastLevel::Success => iced::Color::from_rgba(0.3, 0.8, 0.3, alpha),
            ToastLevel::Warning => iced::Color::from_rgba(0.8, 0.6, 0.2, alpha),
            ToastLevel::Error => iced::Color::from_rgba(0.9, 0.3, 0.3, alpha),
        }
    }
}

struct Toast {
    id: usize,
    title: String,
    body: Option<String>,
    level: ToastLevel,
    created_at: Instant,
    dismiss_at: Instant,
    dismissing_at: Option<Instant>,
}

impl Toast {
    fn alpha(&self, now: Instant) -> f32 {
        // Fade out
        if let Some(fade_start) = self.dismissing_at {
            let elapsed = now.duration_since(fade_start).as_millis() as f32;
            return (1.0 - elapsed / FADE_OUT_MS).max(0.0);
        }
        // Fade in
        let elapsed = now.duration_since(self.created_at).as_millis() as f32;
        if elapsed < FADE_IN_MS {
            // ease-out-cubic: 1 - (1-t)^3
            let t = elapsed / FADE_IN_MS;
            let ease = 1.0 - (1.0 - t).powi(3);
            return ease;
        }
        1.0
    }

    fn is_expired(&self, now: Instant) -> bool {
        if let Some(fade_start) = self.dismissing_at {
            now.duration_since(fade_start).as_millis() as f32 >= FADE_OUT_MS
        } else {
            false
        }
    }
}

pub struct ToastManager {
    toasts: Vec<Toast>,
    next_id: usize,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 0,
        }
    }

    pub fn push(
        &mut self,
        title: impl Into<String>,
        body: Option<String>,
        level: ToastLevel,
        duration: Duration,
    ) {
        let now = Instant::now();
        let toast = Toast {
            id: self.next_id,
            title: title.into(),
            body,
            level,
            created_at: now,
            dismiss_at: now + duration,
            dismissing_at: None,
        };
        self.next_id += 1;
        self.toasts.push(toast);

        // Auto-dismiss oldest if exceeding max
        let active_count = self
            .toasts
            .iter()
            .filter(|t| t.dismissing_at.is_none())
            .count();
        if active_count > MAX_VISIBLE
            && let Some(oldest) = self.toasts.iter_mut().find(|t| t.dismissing_at.is_none())
        {
            oldest.dismissing_at = Some(now);
        }
    }

    pub fn tick(&mut self, now: Instant) {
        // Start fade-out for toasts past their dismiss_at
        for toast in &mut self.toasts {
            if toast.dismissing_at.is_none() && now >= toast.dismiss_at {
                toast.dismissing_at = Some(now);
            }
        }
        // Remove fully expired
        self.toasts.retain(|t| !t.is_expired(now));
    }

    pub fn dismiss(&mut self, id: usize) {
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id)
            && toast.dismissing_at.is_none()
        {
            toast.dismissing_at = Some(Instant::now());
        }
    }

    pub fn has_active_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }

    pub fn view(&self) -> Element<'_, Message> {
        if self.toasts.is_empty() {
            return Space::new().width(0).height(0).into();
        }

        let now = Instant::now();
        let mut toast_views: Vec<Element<'_, Message>> = Vec::new();

        for toast in &self.toasts {
            let alpha = toast.alpha(now);
            let level = toast.level;
            let toast_id = toast.id;

            let mut content_col: Vec<Element<'_, Message>> = Vec::new();

            content_col.push(
                text(&toast.title)
                    .size(13)
                    .color(iced::Color::from_rgba(0.95, 0.95, 0.95, alpha))
                    .into(),
            );

            if let Some(body) = &toast.body {
                content_col.push(
                    text(body)
                        .size(12)
                        .color(iced::Color::from_rgba(0.7, 0.7, 0.7, alpha))
                        .into(),
                );
            }

            let bg_color = level.bg_color(alpha);
            let border_color = level.border_color(alpha);

            let toast_content = container(column(content_col).spacing(2))
                .padding([8, 12])
                .width(Length::Fixed(280.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(bg_color)),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });

            let clickable =
                mouse_area(toast_content).on_press(Message::DismissToast(toast_id));

            toast_views.push(clickable.into());
        }

        let toast_column = column(toast_views).spacing(8);

        // Position top-right
        container(container(toast_column).padding([16, 16]))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .into()
    }
}
