use std::{io::Write, sync::Arc};

use anyhow::Result;
use crossbeam::channel::Sender;
use ratatui::{layout::Rect, prelude::IntoCrossterm};

use super::{
    Backend,
    block::Block,
    iterm2::Iterm2,
    kitty::Kitty,
    sixel::Sixel,
    ueberzug::{Layer, Ueberzug},
};
use crate::{
    config::album_art::ImageMethod,
    ctx::Ctx,
    shared::{events::WorkRequest, macros::status_error, terminal::TERMINAL},
};

#[derive(Debug)]
pub struct AlbumArtFacade {
    image_backend: ImageBackend,
    current_album_art: Option<Arc<Vec<u8>>>,
    current_album_art_is_animated: bool,
    default_album_art: Arc<Vec<u8>>,
    last_size: Rect,
    work_tx: Sender<WorkRequest>,
    is_showing: bool,
    request_queue: Vec<PendingRequest>,
}

#[derive(Debug, Default)]
enum ImageBackend {
    Kitty(Kitty),
    Ueberzug(Ueberzug),
    Iterm2(Iterm2),
    Sixel(Sixel),
    Block(Block),
    #[default]
    None,
}

#[derive(Debug, Default)]
pub enum EncodeData {
    Kitty(<Kitty as Backend>::EncodedData),
    Ueberzug(<Ueberzug as Backend>::EncodedData),
    Iterm2(<Iterm2 as Backend>::EncodedData),
    Sixel(<Sixel as Backend>::EncodedData),
    Block(<Block as Backend>::EncodedData),
    #[default]
    Empty,
}

#[derive(derive_more::Debug, Clone)]
enum PendingRequest {
    Static(#[debug(skip)] Arc<Vec<u8>>),
    Rotated {
        #[debug(skip)]
        data: Arc<Vec<u8>>,
        angle_degrees: f32,
    },
}

impl AlbumArtFacade {
    pub fn new(ctx: &Ctx) -> Self {
        let config = ctx.config.as_ref();
        let image_backend = match config.album_art.method {
            ImageMethod::Kitty => ImageBackend::Kitty(Kitty),
            ImageMethod::UeberzugWayland => ImageBackend::Ueberzug(Ueberzug::new(Layer::Wayland)),
            ImageMethod::UeberzugX11 => ImageBackend::Ueberzug(Ueberzug::new(Layer::X11)),
            ImageMethod::Iterm2 => ImageBackend::Iterm2(Iterm2),
            ImageMethod::Sixel => ImageBackend::Sixel(Sixel),
            ImageMethod::Block => ImageBackend::Block(Block),
            ImageMethod::None => ImageBackend::None,
        };

        Self {
            image_backend,
            current_album_art: None,
            current_album_art_is_animated: false,
            last_size: Rect::default(),
            default_album_art: Arc::new(config.theme.default_album_art.to_vec()),
            work_tx: ctx.work_sender.clone(),
            is_showing: false,
            request_queue: Vec::new(),
        }
    }

    pub fn show_default(&mut self, ctx: &Ctx) -> Result<()> {
        self.use_default_album_art();
        self.show_current(ctx)
    }

    pub fn show_default_rotated(&mut self, angle_degrees: f32, ctx: &Ctx) -> Result<()> {
        self.use_default_album_art();
        self.show_rotated_current(angle_degrees, ctx)
    }

    pub fn show_current(&mut self, ctx: &Ctx) -> Result<()> {
        let Some(current_album_art) = self.current_album_art.as_ref().map(Arc::clone) else {
            log::warn!("Tried to display current album art but none was present");
            return Ok(());
        };

        self.submit_request(PendingRequest::Static(current_album_art), ctx)?;

        Ok(())
    }

    pub fn show_rotated_current(&mut self, angle_degrees: f32, ctx: &Ctx) -> Result<()> {
        let Some(current_album_art) = self.current_album_art.as_ref().map(Arc::clone) else {
            log::warn!("Tried to display rotated album art but none was present");
            return Ok(());
        };

        if self.current_album_art_is_animated || !self.is_kitty_backend() {
            return self.show_current(ctx);
        }

        self.submit_request(PendingRequest::Rotated { data: current_album_art, angle_degrees }, ctx)
    }

    pub fn show(&mut self, data: impl Into<Arc<Vec<u8>>>, ctx: &Ctx) -> Result<()> {
        let data = data.into();
        self.replace_current(data);
        self.show_current(ctx)
    }

    pub fn show_rotated(
        &mut self,
        data: impl Into<Arc<Vec<u8>>>,
        angle_degrees: f32,
        ctx: &Ctx,
    ) -> Result<()> {
        let data = data.into();
        self.replace_current(data);
        self.show_rotated_current(angle_degrees, ctx)
    }

    pub fn current_album_art_is_animated(&self) -> bool {
        self.current_album_art_is_animated
    }

    pub fn has_current_album_art(&self) -> bool {
        self.current_album_art.is_some()
    }

    pub fn is_kitty_backend(&self) -> bool {
        matches!(self.image_backend, ImageBackend::Kitty(_))
    }

    pub fn image_processing_failed(&mut self, err: &anyhow::Error, ctx: &Ctx) -> Result<()> {
        status_error!("Failed to process album art image: {err:?}");

        if self.run_next_queued_request(ctx)? {
            return Ok(());
        }
        Ok(())
    }

    pub fn display(&mut self, data: EncodeData, ctx: &Ctx) -> Result<()> {
        if !self.is_showing {
            log::trace!("Not showing image because its not supposed to be displayed anymore");
            self.request_queue.clear();
            return Ok(());
        }

        if self.run_next_queued_request(ctx)? {
            return Ok(());
        }

        log::debug!(data:?, area:? = self.last_size; "Received encoded data",);

        let w = TERMINAL.writer();
        let mut w = w.lock();
        let w = w.by_ref();
        let c = ctx.config.theme.background_color.map(|c| c.into_crossterm());

        let result = match (&mut self.image_backend, data) {
            (ImageBackend::Kitty(kitty), EncodeData::Kitty(data)) => {
                kitty.hide(w, self.last_size, c).and_then(|()| kitty.display(w, data, ctx))
            }
            (ImageBackend::Ueberzug(ueberzug), EncodeData::Ueberzug(data)) => {
                ueberzug.hide(w, self.last_size, c).and_then(|()| ueberzug.display(w, data, ctx))
            }
            (ImageBackend::Iterm2(iterm2), EncodeData::Iterm2(data)) => {
                iterm2.hide(w, self.last_size, c).and_then(|()| iterm2.display(w, data, ctx))
            }
            (ImageBackend::Sixel(sixel), EncodeData::Sixel(data)) => {
                sixel.hide(w, self.last_size, c).and_then(|()| sixel.display(w, data, ctx))
            }
            (ImageBackend::Block(block), EncodeData::Block(data)) => {
                block.hide(w, self.last_size, c).and_then(|()| block.display(w, data, ctx))
            }
            (ImageBackend::None, EncodeData::Empty) => {
                log::warn!("Tried to display image but no backend is selected");
                Ok(())
            }
            _ => {
                status_error!(
                    "Received encoded data for a different backend than the one in use. Please report this."
                );
                Ok(())
            }
        };

        if let Err(err) = result {
            status_error!("Failed to display image {err:#}");
        }

        Ok(())
    }

    pub fn hide(&mut self, ctx: &Ctx) -> Result<()> {
        self.is_showing = false;
        let w = TERMINAL.writer();
        let mut w = w.lock();
        let w = w.by_ref();
        let c = ctx.config.theme.background_color.map(|c| c.into_crossterm());

        self.request_queue.clear();
        match &mut self.image_backend {
            ImageBackend::Kitty(s) => s.hide(w, self.last_size, c)?,
            ImageBackend::Ueberzug(s) => s.hide(w, self.last_size, c)?,
            ImageBackend::Iterm2(s) => s.hide(w, self.last_size, c)?,
            ImageBackend::Sixel(s) => s.hide(w, self.last_size, c)?,
            ImageBackend::Block(s) => s.hide(w, self.last_size, c)?,
            ImageBackend::None => {}
        }
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        let state = std::mem::take(&mut self.image_backend);
        self.is_showing = false;
        match state {
            ImageBackend::Kitty(kitty) => Box::new(kitty).cleanup(self.last_size),
            ImageBackend::Ueberzug(ueberzug) => Box::new(ueberzug).cleanup(self.last_size),
            ImageBackend::Iterm2(iterm2) => Box::new(iterm2).cleanup(self.last_size),
            ImageBackend::Sixel(s) => Box::new(s).cleanup(self.last_size),
            ImageBackend::Block(s) => Box::new(s).cleanup(self.last_size),
            ImageBackend::None => Ok(()),
        }
    }

    pub fn set_size(&mut self, area: Rect) {
        self.last_size = area;
    }

    fn replace_current(&mut self, data: Arc<Vec<u8>>) {
        self.current_album_art_is_animated = Kitty::is_animated_image(&data).unwrap_or(false);
        self.current_album_art = Some(data);
    }

    fn use_default_album_art(&mut self) {
        self.replace_current(Arc::clone(&self.default_album_art));
    }

    fn submit_request(&mut self, request: PendingRequest, ctx: &Ctx) -> Result<()> {
        self.is_showing = true;
        self.request_queue.push(request);
        if self.request_queue.len() > 1 {
            log::debug!("Image encode request already in flight, queueing the new one.");
            return Ok(());
        }

        let request = self.request_queue.last().cloned().expect("queue to contain one request");
        self.dispatch_request(request, ctx)
    }

    fn dispatch_request(&mut self, request: PendingRequest, ctx: &Ctx) -> Result<()> {
        let max_size = ctx.config.album_art.max_size_px;
        let halign = ctx.config.album_art.horizontal_align;
        let valign = ctx.config.album_art.vertical_align;
        let size = self.last_size;

        match (&mut self.image_backend, request) {
            (ImageBackend::Kitty(_kitty), PendingRequest::Static(data)) => {
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Kitty(Kitty::create_data(
                        &data, size, max_size, halign, valign,
                    )?))
                })))?;
            }
            (ImageBackend::Kitty(_kitty), PendingRequest::Rotated { data, angle_degrees }) => {
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Kitty(Kitty::create_rotated_data(
                        &data,
                        size,
                        max_size,
                        halign,
                        valign,
                        angle_degrees,
                    )?))
                })))?;
            }
            (ImageBackend::Iterm2(_iterm2), PendingRequest::Static(data))
            | (ImageBackend::Iterm2(_iterm2), PendingRequest::Rotated { data, .. }) => {
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Iterm2(Iterm2::create_data(
                        &data, size, max_size, halign, valign,
                    )?))
                })))?;
            }
            (ImageBackend::Sixel(_sixel), PendingRequest::Static(data))
            | (ImageBackend::Sixel(_sixel), PendingRequest::Rotated { data, .. }) => {
                log::debug!("Sending sixel image encode request");
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Sixel(Sixel::create_data(
                        &data, size, max_size, halign, valign,
                    )?))
                })))?;
            }
            (ImageBackend::Block(_block), PendingRequest::Static(data))
            | (ImageBackend::Block(_block), PendingRequest::Rotated { data, .. }) => {
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Block(Block::create_data(
                        &data, size, max_size, halign, valign,
                    )?))
                })))?;
            }
            (ImageBackend::Ueberzug(_ueberzug), PendingRequest::Static(data))
            | (ImageBackend::Ueberzug(_ueberzug), PendingRequest::Rotated { data, .. }) => {
                self.work_tx.send(WorkRequest::ResizeImage(Box::new(move || {
                    Ok(EncodeData::Ueberzug(Ueberzug::create_data(
                        &data, size, max_size, halign, valign,
                    )?))
                })))?;
            }
            (ImageBackend::None, _) => {}
        }

        Ok(())
    }

    fn run_next_queued_request(&mut self, ctx: &Ctx) -> Result<bool> {
        if let Some(request) = self.request_queue.pop()
            && !self.request_queue.is_empty()
        {
            log::debug!("More image requests in queue, encoding the latest one instead");
            self.request_queue.clear();
            self.request_queue.push(request.clone());
            self.dispatch_request(request, ctx)?;
            return Ok(true);
        }

        Ok(false)
    }
}
