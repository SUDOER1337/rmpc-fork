use std::time::Duration;

use anyhow::Result;
use ratatui::{Frame, layout::Rect};
use rmpc_mpd::commands::State;

use super::Pane;
use crate::{
    AppEvent,
    MpdQueryResult,
    config::album_art::AlbumArtEffect,
    core::scheduler::TaskGuard,
    ctx::Ctx,
    shared::{
        album_art::{self, ALBUM_ART},
        events::ClientRequest,
        keys::ActionEvent,
    },
    ui::{UiEvent, image::facade::AlbumArtFacade},
};

#[derive(Debug)]
pub struct AlbumArtPane {
    album_art: AlbumArtFacade,
    is_modal_open: bool,
    fetch_needed: bool,
    rotation_angle_degrees: f32,
    rotation_task: Option<TaskGuard<(crossbeam::channel::Sender<AppEvent>, crossbeam::channel::Sender<ClientRequest>)>>,
}

impl AlbumArtPane {
    pub fn new(ctx: &Ctx) -> Self {
        Self {
            album_art: AlbumArtFacade::new(ctx),
            is_modal_open: false,
            fetch_needed: false,
            rotation_angle_degrees: 0.0,
            rotation_task: None,
        }
    }

    fn rotation_mode_enabled(&self, is_visible: bool, ctx: &Ctx) -> bool {
        is_visible
            && !self.is_modal_open
            && matches!(ctx.config.album_art.effect, AlbumArtEffect::Rotate)
            && self.album_art.is_kitty_backend()
            && matches!(ctx.status.state, State::Play)
    }

    fn should_rotate_current(&self, is_visible: bool, ctx: &Ctx) -> bool {
        self.rotation_mode_enabled(is_visible, ctx)
            && self.album_art.has_current_album_art()
            && !self.album_art.current_album_art_is_animated()
    }

    fn show_current_frame(&mut self, is_visible: bool, ctx: &Ctx) -> Result<()> {
        if self.should_rotate_current(is_visible, ctx) {
            self.album_art.show_rotated_current(self.rotation_angle_degrees, ctx)
        } else {
            self.album_art.show_current(ctx)
        }
    }

    fn show_default_frame(&mut self, is_visible: bool, ctx: &Ctx) -> Result<()> {
        if self.rotation_mode_enabled(is_visible, ctx) {
            self.album_art.show_default_rotated(self.rotation_angle_degrees, ctx)
        } else {
            self.album_art.show_default(ctx)
        }
    }

    fn start_rotation(&mut self, ctx: &Ctx) {
        if self.rotation_task.is_some() || !self.should_rotate_current(true, ctx) {
            return;
        }

        let interval = Duration::from_millis(1000 / u64::from(ctx.config.album_art.rotation_fps));
        self.rotation_task = Some(ctx.scheduler.repeated(interval, |(tx, _)| {
            tx.send(AppEvent::AlbumArtRotationTick)?;
            Ok(())
        }));
    }

    fn stop_rotation(&mut self) {
        self.rotation_task = None;
    }

    fn sync_rotation(&mut self, is_visible: bool, ctx: &Ctx) {
        if self.should_rotate_current(is_visible, ctx) {
            self.start_rotation(ctx);
        } else {
            self.stop_rotation();
        }
    }
}

impl Pane for AlbumArtPane {
    fn render(&mut self, _frame: &mut Frame, area: Rect, _ctx: &Ctx) -> Result<()> {
        self.album_art.set_size(area);
        Ok(())
    }

    fn calculate_areas(&mut self, area: Rect, _ctx: &Ctx) -> Result<()> {
        self.album_art.set_size(area);
        Ok(())
    }

    fn handle_action(&mut self, _event: &mut ActionEvent, _ctx: &mut Ctx) -> Result<()> {
        Ok(())
    }

    fn on_hide(&mut self, ctx: &Ctx) -> Result<()> {
        self.stop_rotation();
        self.album_art.hide(ctx)
    }

    fn resize(&mut self, area: Rect, ctx: &Ctx) -> Result<()> {
        if self.is_modal_open {
            return Ok(());
        }
        self.album_art.set_size(area);
        if self.album_art.has_current_album_art() {
            self.show_current_frame(true, ctx)?;
        }
        self.sync_rotation(true, ctx);
        Ok(())
    }

    fn before_show(&mut self, ctx: &Ctx) -> Result<()> {
        if album_art::fetch_album_art(ctx).is_none() {
            self.show_default_frame(true, ctx)?;
        }
        self.sync_rotation(true, ctx);
        Ok(())
    }

    fn on_query_finished(
        &mut self,
        id: &'static str,
        data: MpdQueryResult,
        is_visible: bool,
        ctx: &Ctx,
    ) -> Result<()> {
        if !is_visible || self.is_modal_open {
            return Ok(());
        }
        match (id, data) {
            (ALBUM_ART, MpdQueryResult::AlbumArt(Some(data))) => {
                if self.rotation_mode_enabled(is_visible, ctx) {
                    self.album_art.show_rotated(data, self.rotation_angle_degrees, ctx)?;
                } else {
                    self.album_art.show(data, ctx)?;
                }
            }
            (ALBUM_ART, MpdQueryResult::AlbumArt(None)) => {
                self.show_default_frame(is_visible, ctx)?;
            }
            _ => {}
        }
        self.sync_rotation(is_visible, ctx);
        Ok(())
    }

    fn on_event(&mut self, event: &mut UiEvent, is_visible: bool, ctx: &Ctx) -> Result<()> {
        match event {
            UiEvent::SongChanged | UiEvent::Reconnected if is_visible => {
                if matches!(event, UiEvent::SongChanged) {
                    self.rotation_angle_degrees = 0.0;
                }
                self.stop_rotation();
                if self.is_modal_open {
                    self.fetch_needed = true;
                    return Ok(());
                }
                self.before_show(ctx)?;
            }
            UiEvent::Displayed if is_visible => {
                if is_visible && !self.is_modal_open {
                    self.show_current_frame(is_visible, ctx)?;
                    self.sync_rotation(is_visible, ctx);
                }
            }
            UiEvent::Hidden if is_visible => {
                self.stop_rotation();
            }
            UiEvent::ModalOpened if is_visible => {
                if !self.is_modal_open {
                    self.stop_rotation();
                    self.album_art.hide(ctx)?;
                }
                self.is_modal_open = true;
            }
            UiEvent::ModalClosed if is_visible => {
                self.is_modal_open = false;

                if self.fetch_needed {
                    self.fetch_needed = false;
                    self.before_show(ctx)?;
                    return Ok(());
                }
                self.show_current_frame(is_visible, ctx)?;
                self.sync_rotation(is_visible, ctx);
            }
            UiEvent::ConfigChanged => {
                if is_visible && !self.is_modal_open {
                    self.show_current_frame(is_visible, ctx)?;
                    self.sync_rotation(is_visible, ctx);
                } else {
                    self.stop_rotation();
                }
            }
            UiEvent::PlaybackStateChanged if is_visible => match ctx.status.state {
                State::Play => {
                    self.show_current_frame(is_visible, ctx)?;
                    self.sync_rotation(is_visible, ctx);
                }
                State::Pause | State::Stop => {
                    self.stop_rotation();
                }
            },
            UiEvent::AlbumArtRotationTick if is_visible => {
                if self.should_rotate_current(is_visible, ctx) {
                    self.rotation_angle_degrees +=
                        ctx.config.album_art.rotation_speed_dps / f32::from(ctx.config.album_art.rotation_fps);
                    if self.rotation_angle_degrees >= 360.0 {
                        self.rotation_angle_degrees %= 360.0;
                    }
                    self.album_art.show_rotated_current(self.rotation_angle_degrees, ctx)?;
                } else {
                    self.stop_rotation();
                }
            }
            UiEvent::Exit => {
                self.stop_rotation();
                self.album_art.cleanup()?;
            }
            UiEvent::ImageEncoded { data } => {
                self.album_art.display(std::mem::take(data), ctx)?;
            }
            UiEvent::ImageEncodeFailed { err } => {
                self.album_art.image_processing_failed(err, ctx)?;
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use crossbeam::channel::{Receiver, RecvTimeoutError, Sender};
    use rmpc_mpd::commands::{Song, State};
    use rstest::rstest;

    use super::AlbumArtPane;
    use crate::{
        AppEvent,
        MpdQueryResult,
        config::{Config, album_art::{AlbumArtEffect, ImageMethod}, tabs::PaneType},
        shared::{
            events::{ClientRequest, WorkRequest},
            mpd_query::MpdQuery,
        },
        tests::fixtures::{app_event_channel, client_request_channel, ctx, work_request_channel},
        ui::{
            UiEvent,
            panes::{Pane, album_art::ALBUM_ART},
        },
    };

    #[rstest]
    #[case(ImageMethod::Kitty, true)]
    #[case(ImageMethod::None, false)]
    fn searches_for_album_art_before_show(
        #[case] method: ImageMethod,
        #[case] should_search: bool,
        app_event_channel: (Sender<AppEvent>, Receiver<AppEvent>),
        work_request_channel: (Sender<WorkRequest>, Receiver<WorkRequest>),
        client_request_channel: (Sender<ClientRequest>, Receiver<ClientRequest>),
    ) {
        let rx = client_request_channel.1.clone();
        let mut ctx = ctx(app_event_channel, work_request_channel, client_request_channel);
        let selected_song_id = 333;
        let mut config = Config::default();
        config.album_art.method = method;
        ctx.config = std::sync::Arc::new(config);
        ctx.queue.push(Song { id: selected_song_id, ..Default::default() });
        ctx.status.songid = Some(selected_song_id);
        ctx.status.state = State::Play;
        let mut screen = AlbumArtPane::new(&ctx);

        screen.before_show(&ctx).unwrap();

        if should_search {
            assert!(matches!(
                rx.recv_timeout(Duration::from_millis(100)).unwrap(),
                ClientRequest::Query(MpdQuery {
                    id: ALBUM_ART,
                    replace_id: Some(ALBUM_ART),
                    target: Some(PaneType::AlbumArt),
                    ..
                })
            ));
        } else {
            assert!(
                rx.recv_timeout(Duration::from_millis(100))
                    .is_err_and(|err| RecvTimeoutError::Timeout == err)
            );
        }
    }

    #[rstest]
    #[case(ImageMethod::Kitty, true)]
    #[case(ImageMethod::None, false)]
    fn searches_for_album_art_on_event(
        #[case] method: ImageMethod,
        #[case] should_search: bool,
        app_event_channel: (Sender<AppEvent>, Receiver<AppEvent>),
        work_request_channel: (Sender<WorkRequest>, Receiver<WorkRequest>),
        client_request_channel: (Sender<ClientRequest>, Receiver<ClientRequest>),
    ) {
        let rx = client_request_channel.1.clone();
        let mut ctx = ctx(app_event_channel, work_request_channel, client_request_channel);
        let selected_song_id = 333;
        let mut config = Config::default();
        config.album_art.method = method;
        ctx.config = std::sync::Arc::new(config);
        ctx.queue.push(Song { id: selected_song_id, ..Default::default() });
        ctx.status.songid = Some(selected_song_id);
        ctx.status.state = State::Play;
        let mut screen = AlbumArtPane::new(&ctx);

        screen.on_event(&mut UiEvent::SongChanged, true, &ctx).unwrap();

        if should_search {
            assert!(matches!(
                rx.recv_timeout(Duration::from_millis(100)).unwrap(),
                ClientRequest::Query(MpdQuery {
                    id: ALBUM_ART,
                    replace_id: Some(ALBUM_ART),
                    target: Some(PaneType::AlbumArt),
                    ..
                })
            ));
        } else {
            let result = rx.recv_timeout(Duration::from_millis(100));
            assert!(result.is_err_and(|err| RecvTimeoutError::Timeout == err));
        }
    }

    #[rstest]
    fn starts_rotation_only_for_kitty_rotate_playing(
        app_event_channel: (Sender<AppEvent>, Receiver<AppEvent>),
        work_request_channel: (Sender<WorkRequest>, Receiver<WorkRequest>),
        client_request_channel: (Sender<ClientRequest>, Receiver<ClientRequest>),
    ) {
        let _app_rx = app_event_channel.1.clone();
        let _work_rx = work_request_channel.1.clone();
        let _client_rx = client_request_channel.1.clone();
        let mut ctx = ctx(app_event_channel, work_request_channel, client_request_channel);
        let mut config = Config::default();
        config.album_art.method = ImageMethod::Kitty;
        config.album_art.effect = AlbumArtEffect::Rotate;
        ctx.config = std::sync::Arc::new(config);
        ctx.status.state = State::Play;

        let mut screen = AlbumArtPane::new(&ctx);
        screen
            .on_query_finished(ALBUM_ART, MpdQueryResult::AlbumArt(Some(vec![1, 2, 3])), true, &ctx)
            .unwrap();

        assert!(screen.rotation_task.is_some());
    }

    #[rstest]
    fn does_not_rotate_for_non_kitty_backends(
        app_event_channel: (Sender<AppEvent>, Receiver<AppEvent>),
        work_request_channel: (Sender<WorkRequest>, Receiver<WorkRequest>),
        client_request_channel: (Sender<ClientRequest>, Receiver<ClientRequest>),
    ) {
        let _app_rx = app_event_channel.1.clone();
        let _work_rx = work_request_channel.1.clone();
        let _client_rx = client_request_channel.1.clone();
        let mut ctx = ctx(app_event_channel, work_request_channel, client_request_channel);
        let mut config = Config::default();
        config.album_art.method = ImageMethod::Sixel;
        config.album_art.effect = AlbumArtEffect::Rotate;
        ctx.config = std::sync::Arc::new(config);
        ctx.status.state = State::Play;

        let mut screen = AlbumArtPane::new(&ctx);
        screen
            .on_query_finished(ALBUM_ART, MpdQueryResult::AlbumArt(Some(vec![1, 2, 3])), true, &ctx)
            .unwrap();

        assert!(screen.rotation_task.is_none());
    }

    #[rstest]
    fn song_change_resets_angle_and_pause_stops_rotation(
        app_event_channel: (Sender<AppEvent>, Receiver<AppEvent>),
        work_request_channel: (Sender<WorkRequest>, Receiver<WorkRequest>),
        client_request_channel: (Sender<ClientRequest>, Receiver<ClientRequest>),
    ) {
        let _app_rx = app_event_channel.1.clone();
        let _work_rx = work_request_channel.1.clone();
        let _client_rx = client_request_channel.1.clone();
        let mut ctx = ctx(app_event_channel, work_request_channel, client_request_channel);
        let mut config = Config::default();
        config.album_art.method = ImageMethod::Kitty;
        config.album_art.effect = AlbumArtEffect::Rotate;
        ctx.config = std::sync::Arc::new(config);
        ctx.status.state = State::Play;

        let mut screen = AlbumArtPane::new(&ctx);
        screen.rotation_angle_degrees = 123.0;
        screen
            .on_event(&mut UiEvent::SongChanged, true, &ctx)
            .unwrap();
        assert_eq!(screen.rotation_angle_degrees, 0.0);

        screen.rotation_task = Some(ctx.scheduler.repeated(Duration::from_millis(100), |_| Ok(())));
        ctx.status.state = State::Pause;
        screen
            .on_event(&mut UiEvent::PlaybackStateChanged, true, &ctx)
            .unwrap();
        assert!(screen.rotation_task.is_none());
    }
}
